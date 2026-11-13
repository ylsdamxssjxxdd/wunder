---
title: Multi-threading
summary: Scheduling, locks, and realtime state isolated per thread inside one process: the Desktop frontend's local waiting queue and the Cloud engine session queue, a 120-second session lock TTL, tool parallelism ≤ 4, and a round cap of 1000.
updated_at: 2026-10-16
---

# Multi-threading

A single user can hold several task threads at once, and scheduling, locks, and realtime state are isolated per thread so the threads do not block one another.

## Concurrency control

- **Queueing has two layers**: since Desktop v1.0.0 the frontend local waiting queue (`waiting_turns`) holds messages sent during a run above the input box and sends them when the current turn settles; the Cloud build still uses the engine-level session queue (on by default, in-order execution).
- A **session lock** has a TTL of 120 seconds with heartbeat renewal; when its holder exits abnormally, the TTL releases it.
- **Tool parallelism ≤ 4**: tools that run in parallel take a read lock, and tools that run exclusively take a write lock.
- **Round caps, active-session caps, daily quotas, and retry circuit breaking** form four hard constraints.

## Defaults

| Item | Value |
| --- | --- |
| Session lock TTL | 120 seconds (with heartbeat) |
| Tool parallelism | ≤ 4 |
| Round cap | 1000 by default |

## Limits

- With `agent_queue.enabled` turned off, the Cloud build judges a message sent during a run as "user busy" and no longer shows it in the queue bar (the Server warns about this at startup). This is a configuration choice, not a fault; the Desktop build is unaffected and uses the frontend local waiting queue.
- Blocking category quotas and bounded queues are hard caps: when full, the system reports busy and does not wait in a queue.
