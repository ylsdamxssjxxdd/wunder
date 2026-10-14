---
title: Observability
summary: Where to start looking when something breaks — what logs, runtime metrics and alerts, monitoring and thread traces, and performance testing each tell you, and the order to work through a failure from the error message onward.
read_when:
  - A request, session, or channel failed and you need the first place to look
  - You need to size log retention, alert thresholds, or a stress test before deploying
  - You are deciding whether a problem is load, connection, or per-thread behavior
source_docs:
  - crates/wunder-runtime/src/core/logging.rs
  - crates/wunder-core/src/config.rs
  - crates/wunder-runtime/src/core/runtime_metrics.rs
  - crates/wunder-runtime/src/api/ws_log.rs
  - crates/wunder-runtime/src/channels/service.rs
  - crates/wunder-runtime/src/api/user_channel_logs.rs
  - crates/wunder-runtime/src/ops/throughput.rs
  - crates/wunder-runtime/src/services/stress_thread.rs
updated_at: 2026-10-13
---

When something goes wrong, the clues are concentrated in four places: **logs** record what happened, **runtime metrics and alerts** show how loaded the system is right now, **monitoring and thread traces** reconstruct what a particular thread went through, and **performance testing** tells you how much the system can take. The four are independent and each covers its own area, and following them in this order is the fastest way to work a problem through.

![Observability diagram: three-layer separation of fact layer, replay layer, and profile layer](/docs/assets/manual/core-observability.svg)

## Logs

Logs go to two channels at once: a colorized layer on the console and a file layer with one JSON object per line. Files roll daily, their names begin with `server.jsonl`, and by default they sit under `./config/data/logs/server`.

The default level is `info`, and it can be overridden with the `RUST_LOG` environment variable; noise from third-party libraries is quieted out of the box.

Retention defaults to **0, which means files are never cleaned up automatically** — the system only deletes old files by day once you set an explicit positive number of days. Long-running deployments should either set retention explicitly or rotate the log directory themselves. Note that in sandbox mode nothing is written to files, and only console output is produced. The Helm (cli) logs at `warn` and above by default, the standalone local bridge at `info`/`warn`, and neither writes log files.

## Runtime metrics and alerts

The admin side exposes a runtime metrics snapshot with fixed categories: blocking pools are split by database, file, CPU, and external call, each reporting call counts, queue waits, and timeouts; bounded queues report backlog and busy rejections; long-running background tasks report their running counts and any alerts; and the scheduling loop reports whether its heartbeat has stalled.

Alerts carry a threshold and a severity. For a given blocking pool, timeout-class alerts (`critical`) are evaluated before latency-class ones (`warning`), so when you see a latency warning, first confirm there is no timeout still in queueing or execution.

## WebSocket and channel event logs

WebSocket connections produce structured logs under one target, covering seven stages: `open`, `ready`, `handshake`, `handshake_error`, `message`, `parse_error`, and `close`. A connection that will not open, a rejected handshake, or a message that fails to parse can each be told apart by which stage it reached.

Channel runtime logs live in an in-memory ring buffer that keeps only the most recent 300 entries; identical content is deduplicated inside a 20-second flood window, nothing is written to the database, and a process restart clears the buffer. They can be queried two ways: the user side reads a user's own channel logs (`/wunder/channels/runtime_logs`, with `/probe` for an active probe and `/wunder/channels/reconnect` for a reconnect), and the admin side reads all of them (`/wunder/admin/channels/runtime_logs`, which also supports `/probe`). Use a probe for active connectivity and a reconnect for long-connection failures. Anything that needs long-term auditing has to be shipped to an external log system.

## Monitoring and thread traces

Admin-side monitoring is the entry point to governance views. Sessions are labeled by status — running, waiting, queued, finished, error, cancelling, and cancelled — and you can export a session's thread log, page through its turns, or pull a snapshot (for example, `/wunder/admin/monitor/{session_id}/thread-log/export|turns|snapshot|changes`).

The log pane on the right side of the Bridge debug panel is the **thread trace view**; following a single thread's execution record starts there.

Not everything has a Bridge panel: interlink governance, long-term memory, and goals are backend-only, with no Bridge UI, so those are checked through their APIs and the logs.

## Three performance entry points

| Entry point | What it is for | Key limits |
| --- | --- | --- |
| Throughput | Stressing the model's streaming capacity, as concurrency × input/output tokens | Concurrency ≤ 1024; input presets 1024…1,048,576 and output presets 1024…8192, with hard caps of 16,777,216 input and 1,048,576 output; 50 runs of history retained |
| Performance sampling | A health check in a real environment, sampling commands, IO, and so on | Uses one fixed administrator sampling user; data lands in the workspace at `.wunder_perf`; a single command times out after 5 seconds |
| Thread rendering stress test | Generating a large volume of round data to test front-end rendering and projection | User rounds ≤ 2000 and model rounds ≤ 2000, total entries ≤ 6,000,000; concurrent tracking jobs ≤ 8; generation timeout capped at 1 hour |

Throughput and performance sampling belong to the admin side (`/wunder/admin/throughput/*`, `/wunder/admin/performance/sample`). Throughput's live viewing channel (`/wunder/throughput/ws`) is deliberately exempt from the admin gate and is guarded instead by a single-use ticket. The thread rendering stress test deliberately sits on the user side (`POST /wunder/chat/stress-threads`) so it can be called with ordinary user credentials for self-testing, but a single job's data ceiling is very high, so watch for pressure under heavy load. The three differ in target and in what they measure — network and model pressure, environment sampling, and front-end data volume — so they should not be treated as one and the same panel.

## How performance is scored

The three forms — the Pupa, the Beehive, and the Hull — are each scored **independently** on a weighted 0–100 scale, and scores cannot be added across forms. Comparisons are always made in the same environment, through the same entry point, before and after a change; the matching baselines live in the documentation's performance-baseline section. Numbers obtained on a different machine or through a different entry point are not comparable.

## Troubleshooting order

1. **Get the structured error.** When an interface fails, it returns one error object carrying an error code, a description, the HTTP status, an actionable hint for the user, and this request's trace id and timestamp; the trace id and error code are also written back in the response headers. Note the trace id first.
2. **Check monitoring.** Look at a session's status and turns; when a thread seems stuck, first tell apart "waiting for a person" from "queueing or waiting on a lock".
3. **Match it against the logs.** Search the log files by trace id; for live-connection problems, filter on the WebSocket stages; for a channel that is not getting through, check the runtime logs and the probe result.
4. **Go into the trace.** In the thread trace view, check each round's model calls, tool batches, and results to find the first point of divergence.

## Related pages

- [Stream Events](/docs/en/capabilities/stream-events/) — WebSocket event fields
- [Bridge Panel Guide](/docs/en/capabilities/admin-panels/) — entry points for the monitoring and debug panels
