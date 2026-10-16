---
title: Stability
summary: Four kinds of hard constraint — bounded queues, limits and budgets, session locks with a single writer, and circuit-breaker thresholds — plus a uniform error structure and a symptom-to-action table.
updated_at: 2026-10-16
---

Stability rests on three things: **containing the spread** (one failing link does not bring down the others), **being able to recover** (failures have a formal recovery path), and **being able to review** (errors are structured and classifiable). It comes down to four kinds of hard constraint: bounds, budgets, circuit breaking, and a single writer.

![Stability diagram: risk sources pass through isolation governance and recovery paths to prevent cascading failures and form a regressable system](/docs/assets/manual/core-stability.svg)

## Bounded queues

Every queue has a fixed capacity. A write that fails and times out returns "queue busy" and records a busy metric. There is **no unbounded buffer and no silent drop**.

| Queue | Bound |
| --- | --- |
| Per-subscription event write queue | 256 |
| Slow-client watermark | remaining capacity ≤ 2 |
| Single event fetch | 200 |
| Thread changes per page | 200 |
| Channel ingress | capacity 512 / concurrency 8 / enqueue wait 80 ms |
| Workspace write queue | 2048 |

Background tasks all start with a tag; a long run raises an alert, and one loop crashing does not take down the process. When troubleshooting, the task tag in the logs tells you which loop it was.

## Limits and budgets

| Item | Value |
| --- | --- |
| User input cap | 1,048,576 characters; image attachments not counted |
| Default model call timeout | 120 seconds |
| Default tool timeout | 120 seconds (floor of 1 second) |
| Tool concurrency | ≤ 4 |
| Turn cap | 1000 by default for non-admins (floor of 2) |
| Tool-call budget per turn | 10000 |
| Memory-recall budget per turn | 2000 |
| Session lock lifetime / heartbeat | 120 seconds / 5 seconds |
| Active session cap | 300 by default (constrains non-admins) |
| Approval cache | 600 seconds |

Session queueing is on by default. With it turned off, sending a message while a run is in progress is immediately judged "user busy" and no queue entry is created either (the Hull warns about this at startup). This is a configuration trade-off, not a failure.

## Session locks and the single writer

**The same task thread executes serially, and different threads do not block each other**: scheduling, locking, and realtime state are all isolated per thread.

A session lock is a persistent lock with a 120-second lifetime, renewed by a heartbeat every 5 seconds. Waiters back off rather than polling on a fixed interval: backoff starts after two attempts, is capped at 3 seconds, and includes jitter; a wait longer than 30 seconds raises an alert. A fixed 0.2-second poll is deliberately avoided here, since a saturated system would otherwise turn its own polling into a storage storm.

**The single-writer principle**: the thread log has exactly one write path. On submission:

1. Each session first passes through a gate that serializes it, so change order matches submission order;
2. The storage call is one short blocking transaction;
3. **The cursor is published to the change channel only after the transaction succeeds** — the channel carries a wake-up signal, not the truth;
4. Idempotent no-ops (a replayed acceptance, an unchanged state, duplicate feedback) produce no change and no notification; a failed submission never publishes a cursor.

Callers must not assemble their own cursors, and must not notify the change channel before the transaction completes. This principle keeps "the order the client sees" consistent with "the order written to storage", and it is what makes lossless catch-up possible after a disconnect.

Long-held locks and slow clients are two problems called out for governance: a lock is covered by its lifetime plus heartbeat in case the holder disappears, and a slow client is detected by the watermark, sent a `resume_recommended` first, and then uncoupled (see [Realtime](/docs/en/concepts/core-realtime/)).

## Recovery and circuit breaking

| Scenario | Mechanism |
| --- | --- |
| Streaming stalls | The idle watchdog judges by how long there has been no activity, not by total elapsed time |
| The model returns empty or unusable output | Retried once with thinking off; if it still fails, an output error is reported |
| Context over its limit | Progressive compression (triggered at three points: history, overflow, and pre-sampling); overflow recovery at most 8 times |
| The same kind of failure recurring | Circuit broken by failure fingerprint |
| Network or subscription disconnect | Cursor catch-up, without replaying the whole history |

Retry circuit breaking counts by the same fingerprint; the thresholds are:

| Condition | Threshold |
| --- | --- |
| Non-retryable failure | 5 times |
| Parameter or structure failure | 2 times (a deterministic error will not improve by waiting, so after two it gives a hint to change course) |
| Retryable failure / the same tool failing repeatedly | 5 times |
| Successes without progress | 4 times |

A fingerprint keeps at most 240 characters of detail. **Circuit breaking looks at how many times the same problem repeats, not at how many calls were made in total**, so a ten-thousand-call tool budget will not be burned away by a single infinite loop.

## Error structure

Every interface error uses the same structure: it marks the failure and carries an error code, a description for the user, the corresponding HTTP status, an actionable hint, and a trace id and timestamp for tracking, with the trace id and error code also written back in the response headers.

**Judge by the error code, not by the message text** — message text changes with language and version, whereas the mapping between error code and HTTP status is the contract.

## Symptoms and actions

| What you see | What the system is doing | What to do |
| --- | --- | --- |
| Shows compacting or queued | Normal processing | Wait |
| `slow_client` or `resume_recommended` appears | The subscription was uncoupled to protect the stream | Rebuild the projection through the catch-up endpoints |
| Judged "user busy" | That task thread is still executing (the session lock is held) | Queue, or yield/cancel that thread |
| Stopped after the same tool failed repeatedly | Fingerprint circuit breaking has tripped | Change the parameters or the path; resubmitting is useless |
| Reports an output error | The one-time recovery has been used up | Change the model configuration or rephrase |
| Reports insufficient quota | The admission-stage deduction did not pass | See [Quota and Token Usage](/docs/en/concepts/quota-and-token-usage/) |
| The UI and backend state disagree | The projection is behind | Refresh or re-enter; if it still disagrees, look at the runtime state |
| You need to start completely over | — | Use "reset work state" (cancels the user's sessions and tasks, clears the workspace, and returns a brand-new session) |
