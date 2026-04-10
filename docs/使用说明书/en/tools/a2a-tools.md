---
title: A2A Tools
summary: The current return structures of `a2a@service`, `a2a_observe`, and `a2a_wait`.
read_when:
  - You need to call an external A2A service and track task status
source_docs:
  - src/services/tools.rs
updated_at: 2026-04-10
---

# A2A Tools

A2A is currently a small tool group:

- `a2a@service-name`
- `a2a_observe`
- `a2a_wait`

## `a2a@service-name`

This submits a task and usually returns an accepted state:

```json
{
  "ok": true,
  "action": "a2a_send",
  "state": "accepted",
  "summary": "Submitted task task_xxx to A2A service helper.",
  "data": {
    "endpoint": "https://a2a.example.com",
    "service_name": "helper",
    "task_id": "task_xxx",
    "context_id": "ctx_xxx",
    "status": "submitted",
    "answer": null
  }
}
```

## `a2a_observe`

This checks the current snapshot:

```json
{
  "ok": true,
  "action": "a2a_observe",
  "state": "running",
  "summary": "Observed 2 A2A tasks; 1 still pending.",
  "data": {
    "tasks": [ ... ],
    "pending": [ ... ],
    "pending_total": 1,
    "timeout": false
  }
}
```

## `a2a_wait`

This waits for a period of time until completion or timeout:

```json
{
  "ok": true,
  "action": "a2a_wait",
  "state": "running",
  "summary": "Observed 2 A2A tasks; 1 still pending.",
  "data": {
    "tasks": [ ... ],
    "pending": [ ... ],
    "pending_total": 1,
    "elapsed_s": 1.25,
    "timeout": true
  },
  "next_step_hint": "Call a2a_wait again or inspect the pending tasks before assuming the A2A workflow is complete."
}
```

## Key points

- `a2a@service-name` starts the work
- `a2a_observe` checks the snapshot
- `a2a_wait` performs the waiting loop
