---
title: Monitoring and Benchmark
summary: Wunder's observability surface is not a single monitoring page, but composed of session monitoring, tool statistics, performance sampling, throughput stress testing, and benchmark together.
read_when:
  - You are troubleshooting thread, tool, or model pipeline issues
  - You need to distinguish the responsibilities of monitor, throughput, performance, and benchmark
source_docs:
  - docs/API文档.md
  - docs/设计方案.md
  - src/api/admin.rs
---

# Monitoring and Benchmark

Wunder has separated "can we see the problem" and "can we quantify the problem" into several independent pipelines.

## Key Points on This Page

This page only clarifies:

- Where to look for online thread and tool issues
- The difference between performance sampling and throughput stress testing
- Why benchmark is not just a simple stress test

## Four Categories of Capabilities

### Server Runtime Logs

The server version `wunder-server` additionally writes runtime logs to the local `config/data/logs/server/` directory.

- Only the server variant writes local logs; desktop and cli do not write here.
- Files are daily-rotated JSONL, retained for 14 days by default, making it easy to grep directly, archive, or connect to collection systems.
- When the service exits abnormally, first check the latest logs for `panic`, `server exited unexpectedly`, HTTP 5xx / failure records.

### Session Monitoring

Main endpoints:

- `GET /wunder/admin/monitor`
- `GET /wunder/admin/monitor/{session_id}`
- `POST /wunder/admin/monitor/{session_id}/cancel`
- `POST /wunder/admin/monitor/{session_id}/compaction`

This pipeline addresses:

- What the thread is doing right now
- What events occurred recently
- Current token usage, stage, and duration

### Tool Usage Statistics

Main endpoint:

- `GET /wunder/admin/monitor/tool_usage`

It addresses:

- Which tool was used the most recently and by whom
- Whether there's a correlation between tool calls and thread state

### Performance and Throughput

Main endpoints:

- `/wunder/admin/throughput/*`
- `/wunder/admin/performance/sample`

These are not the same thing:

- Throughput is more about concurrent stress testing
- Performance sampling is more about pipeline baseline sampling, not involving model capability evaluation

### Benchmark

Main endpoint:

- `/wunder/admin/benchmark/*`

It's closer to "capability evaluation," focusing on task completion quality and result structure, not just speed.

## Why Separate Them

Because these types of problems are inherently different:

- Online thread anomalies: check monitor
- Whether a tool has become a hotspot or bottleneck: check tool_usage
- Whether the service can handle high concurrency: check throughput
- Whether a change caused capability regression: check benchmark

## Key Fields to Remember When Observing

- `trace_id`: Cross-module tracing
- `log_profile`: `normal` or `debug`
- `round_usage.total_tokens`: Actual context usage
- `prefill_*` / `decode_*`: Speed and duration breakdown

## Common Misconceptions

### Using benchmark as a substitute for online monitoring

Benchmark cannot replace real thread monitoring.

### Using throughput stress testing as a substitute for capability evaluation

Throughput only tells you "can it handle the load," not "is it answering well."

### Only looking at one layer of logs

Many issues require looking at monitor, tool_usage, and channel runtime together.

## Implementation Recommendations

- Use `monitor` for online threads.
- Use `tool_usage` for tool hotspots and call coverage.
- Use `throughput/performance` for system pipeline load capacity.
- Use `benchmark` for task quality and capability regression.

## Further Reading

- [Channel Runtime](/docs/en/ops/channel-runtime/)
- [Stream Events Reference](/docs/en/reference/stream-events/)
- [Admin Panels Index](/docs/en/reference/admin-panels/)