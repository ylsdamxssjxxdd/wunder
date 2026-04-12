---
title: Agent Swarm
summary: The multi-agent collaboration tool for dispatching agents that already exist.
read_when:
  - You need to dispatch other formal agents the current user already owns
source_docs:
  - src/services/tools.rs
updated_at: 2026-04-12
---

# Agent Swarm

The biggest difference between `agent_swarm` and `subagent_control` is:

- `subagent_control`: temporarily spawn child runs inside the current session
- `agent_swarm`: dispatch other formal agents that already exist for the user

## Main actions

- `list`
- `send`
- `batch_send`
- `spawn`
- `wait`
- `status`
- `history`

## `list`

```json
{
  "ok": true,
  "action": "list",
  "state": "completed",
  "summary": "Listed 4 swarm workers.",
  "data": {
    "total": 4,
    "items": [ ... ]
  }
}
```

## `spawn`

A typical result is an asynchronous accepted state:

```json
{
  "ok": true,
  "action": "spawn",
  "state": "accepted",
  "summary": "Spawned swarm task ...",
  "data": {
    "task_id": "task_xxx",
    "run_id": "run_xxx",
    "session_id": "sess_xxx",
    "agent_id": "agent_research",
    "agent_name": "Researcher",
    "created_session": true
  },
  "next_step_hint": "Use agent_swarm.wait or status/history before treating the worker result as final."
}
```

## Thread strategy

- The default is `threadStrategy=fresh_main_thread`: the worker starts from a clean new thread and that thread becomes its new main thread.
- If you want the worker to keep writing into its long-lived main thread, use `threadStrategy=main_thread` or `reuseMainThread=true`.
- `main_thread` means "reuse the worker's current main thread, or create and bind one first if it does not exist yet".
- For `send` / `batch_send`, an explicit `sessionKey` still has the highest priority and pins the run to that exact thread.
- `spawn` supports the same thread strategy arguments and forwards them to the underlying worker dispatch.

## `status`

```json
{
  "ok": true,
  "action": "status",
  "state": "completed",
  "summary": "Loaded swarm status for Researcher.",
  "data": {
    "agent": { ... },
    "session_total": 6,
    "active_session_total": 2,
    "running_session_total": 1,
    "lock_session_total": 0,
    "active_session_ids": ["sess_xxx"],
    "recent_sessions": [ ... ]
  }
}
```

## State semantics

Common `state` values in `agent_swarm`:

- `completed`
- `accepted`
- `running`
- `timeout`
- `partial`
- `cancelled`
- `error`

When you see `accepted`, `running`, or `timeout`, you usually should not treat the result as final. Continue with `wait` or `status/history`.
