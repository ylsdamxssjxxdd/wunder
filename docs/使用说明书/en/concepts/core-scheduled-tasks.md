---
title: Scheduled Tasks
summary: The semantics and bounds of the at, every, and cron schedule kinds, the two delivery targets, lease, backoff, and auto-disable parameters, and support by form.
read_when:
  - You want a report, inspection, or reminder to run on a schedule instead of by hand
  - A scheduled task did not run, ran twice, or was disabled on its own
  - You need to know which forms support scheduling and which do not
source_docs:
  - crates/wunder-runtime/src/services/cron.rs
  - crates/wunder-runtime/src/services/cron_schedule.rs
  - crates/wunder-runtime/src/services/cron/policy.rs
  - crates/wunder-runtime/src/api/cron.rs
updated_at: 2026-10-16
---

The task definition is persisted in storage, and a background loop claims work by due time, independent of which session you happen to be viewing. Every run is recorded (`/wunder/cron/runs`): whether it ran, whether it succeeded, and when it will run next; failures come with backoff and auto-disable; and a process restart does not lose the task definitions.

This differs from waiting inside a session: yielding in a session only affects the current thread, whereas a scheduled task creates a new run in the future.

![Scheduled tasks diagram: scheduling rules enter the scheduler, then dispatch to background execution and execution records](/docs/assets/manual/core-scheduled-tasks.svg)

## Schedule kinds

The `kind` field is one of `at`, `every`, or `cron`.

| Kind | Semantics | Input | Bounds |
| --- | --- | --- | --- |
| `at` | One-off point in time | An RFC3339 timestamp | At most 365 days ahead; auto-disabled once it runs (disabled with a reason if it fails) |
| `every` | Fixed interval | `every_ms` (optional start time) | 1,000 ms ≤ interval ≤ 86,400,000 ms (24 hours) |
| `cron` | Recurring expression | A 5–7 field expression plus an optional timezone | Fields accept only digits and `* / - , ?`; the timezone is parsed as a named timezone, defaulting to the local timezone |

Natural language also works in tools and quick input, and the system converts it into the kinds above: `every 5 minutes` becomes a fixed interval, and `daily at 9am` and `weekdays at 6pm` become the corresponding cron expressions; when it cannot parse the input, it suggests the formats that are accepted.

Content limits: a task name is at most 128 characters, and a trigger message at most 16,384 characters.

## Result delivery

`session_target` has two values.

| Value | Behavior |
| --- | --- |
| `main` (default) | The result is delivered to the bound current session, continuing the same conversation |
| `isolated` | Each run creates a separate thread session and keeps a link back to the parent session |

Choose `isolated` for recurring tasks that need a trail and should not disturb the main conversation; choose `main` when you want the result appended directly to the existing context.

## Execution guarantees

In multi-instance deployments a **lease** prevents duplicate execution: when a task is claimed, the runner's identity, a run token, and a lease expiry are written, and the lease is renewed with a heartbeat while the run lasts; if the process crashes, the lease expires and another instance can claim the task. Each record also carries a deduplication key and a consecutive-failure count, and writes down why it was disabled.

Default scheduling parameters:

| Parameter | Default |
| --- | --- |
| Max concurrent runs | 1 |
| Poll interval | 1000 ms |
| Max idle sleep | 5000 ms |
| Lease lifetime / heartbeat | 300 seconds / 60 seconds |
| Consecutive-failure auto-disable threshold | 5 |

After a failure, backoff steps up with the consecutive-failure count: **30 seconds → 60 seconds → 5 minutes → 15 minutes → 1 hour**, then stays at the last step. Once the failure limit is reached, the task is auto-disabled with a written reason; re-enabling it resets the failure count.

## Master switch

With the master switch off (`cron.enabled=false`), four kinds of write operation are rejected: `add`, `update`, `enable`, and `run`, each with a clear message. Status, list, a single detail, run records, delete, and disable all remain available, so existing tasks can still be reviewed and cleaned up.

## Entry points and support by form

| Surface | Status |
| --- | --- |
| API | `/wunder/cron/*`, 11 endpoints in all (status / list / runs / add / update / remove / enable / disable / get / run / action), on the **user side**, isolated by user ownership |
| Tool | The built-in scheduled-tasks tool (canonical name `定时任务`, alias `schedule_task`), sharing the same handler as the API, so the model can create tasks on its own |
| Beehive | A scheduled-tasks category page under settings |
| Pupa | A scheduled-tasks page plus a native entry point, calling the same handler in-process |
| Helm | **No scheduled tasks**: the local engine explicitly disables the scheduler at startup, and there is no related slash command |
| Bridge | No user-level task management page; it exists only as a template configuration under preset agents |

When you need something to run on a schedule, configure it on the Beehive, the Pupa, or the Hull side.
