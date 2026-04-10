---
title: Subagent Control
summary: The actions, waiting semantics, state semantics, and return structure of `subagent_control`.
read_when:
  - You need to spawn temporary child agents inside the current session
source_docs:
  - src/services/tools/subagent_control.rs
updated_at: 2026-04-10
---

# Subagent Control

`subagent_control` is now a clear multi-action tool rather than the older simple "spawn a child thread" interface.

## When it fits

Good fit:

- temporarily spawning child agents inside the current main-agent session
- inspecting, waiting for, interrupting, closing, or resuming those child runs
- performing one or more rounds of delegated collaboration

Not a fit:

- dispatching other formal agents that already belong to the user  
That belongs to [Agent Swarm](/docs/en/tools/agent-swarm/)

## Main actions

- `list`
- `history`
- `send`
- `spawn`
- `batch_spawn`
- `status`
- `wait`
- `interrupt`
- `close`
- `resume`

## `spawn`

This action starts a new child-agent run.

A typical result is "accepted but not finished":

```json
{
  "ok": true,
  "action": "spawn",
  "state": "accepted",
  "summary": "Spawned child run ...",
  "data": {
    "run_id": "run_xxx",
    "session_id": "sess_xxx",
    "status": "accepted"
  },
  "next_step_hint": "Use subagent_control.wait/status/history before treating unfinished child runs as complete."
}
```

## `list`

```json
{
  "ok": true,
  "action": "list",
  "state": "completed",
  "summary": "Found 3 child sessions.",
  "data": {
    "total": 3,
    "items": [
      {
        "dispatch_id": "dispatch_xxx",
        "run_id": "run_xxx",
        "session_id": "sess_xxx",
        "status": "running",
        "terminal": false,
        "failed": false,
        "agent_id": "worker-a",
        "label": "Research materials",
        "elapsed_s": 12.3,
        "result_preview": null,
        "error": null
      }
    ]
  }
}
```

## `history`

```json
{
  "ok": true,
  "action": "history",
  "state": "completed",
  "summary": "Loaded 18 messages from child session history.",
  "data": {
    "session_id": "sess_xxx",
    "messages": [ ... ]
  }
}
```

## `status` and `wait`

These are the two most important actions.

### `status`

Snapshot only, without blocking:

```json
{
  "ok": true,
  "action": "status",
  "state": "running",
  "summary": "1 child runs are still active.",
  "data": {
    "status": "running",
    "items": [ ... ],
    "selected_items": [ ... ]
  }
}
```

### `wait`

This action polls and waits. Depending on what happens, `state` may become:

- `completed`
- `running`
- `timeout`
- `partial`

It often includes:

```json
{
  "next_step_hint": "Use subagent_control.wait/status/history before treating unfinished child runs as complete."
}
```

## `interrupt`, `close`, and `resume`

These actions focus on which child sessions were updated:

```json
{
  "ok": true,
  "action": "interrupt",
  "state": "completed",
  "summary": "interrupt updated 1 child sessions.",
  "data": {
    "updated_total": 1,
    "items": [
      {
        "session_id": "sess_xxx",
        "status": "cancelling"
      }
    ]
  }
}
```

## Key interpretation

- `accepted` does not mean completed
- `status` is for snapshots
- `wait` is for convergence
- if `next_step_hint` exists, the system is explicitly telling you to follow up rather than wrap up
