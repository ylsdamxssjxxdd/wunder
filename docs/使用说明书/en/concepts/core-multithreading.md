---
title: Multi-threading
summary: The Multi-threading core raises throughput and resource use through concurrency control inside a single process: one user can hold several concurrent task threads, backed by a session queue, session locks, a parallelism cap, and a daily quota.
updated_at: 2026-10-13
---

# Multi-threading

The Multi-threading core raises throughput and resource use through concurrency control inside a single process: a single user can hold several task threads at once, and scheduling, locks, and realtime state are isolated per thread.

## How it works

- The **engine-level session queue** is enabled by default, with `会话让出` as the way to yield.
- A **session lock** has a TTL of 120 seconds with heartbeat renewal; when its holder exits abnormally, the TTL releases it.
- **Tool parallelism ≤ 4**: tools that run in parallel take a read lock, and tools that run exclusively take a write lock.
- **Round caps, active-session caps, daily quotas, and retry circuit breaking** form four hard constraints.

## Key numbers

| Item | Value |
| --- | --- |
| Session lock TTL | 120 seconds (with heartbeat) |
| Tool parallelism | ≤ 4 |
| Round cap | 1000 by default |

## Boundaries and current limits

- With `agent_queue.enabled` turned off, sending another message while a run is in progress is judged "user busy" and no longer enters the queue bar (the Hull warns about this at startup). This is a configuration choice, not a fault.
- Blocking category quotas and bounded queues are hard caps: when full, the system reports busy and does not wait in a queue.
