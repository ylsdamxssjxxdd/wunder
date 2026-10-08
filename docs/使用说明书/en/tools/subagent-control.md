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

- Users own exactly one agent instance, so there is no pool of existing agents to dispatch between; spawn a subagent when work can run in parallel instead.

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


## Task tree and bounded background

All selectors are restricted to the same user and root task tree. Independent user forks remain a separate scope. `list` defaults to direct children; `parent_id=/root` selects the root children. Results expose stable `root_session_id` and `task_path`; `session_id` also accepts that path. Same-tree workers can send guidance/tasks to one another. Reports still target the durable direct parent; a new dispatched task reports completion to its dispatcher without reparenting the worker.

`spawn` and `batch_spawn` accept `fork_turns` (integer 0–16, default 0) and `context_summary` (at most 16 KiB UTF-8). Per-task values override batch defaults; batches accept at most 64 tasks and validate all background before dispatch. Only visible user/assistant text is copied, grouped by root user turn, with at most 256 items, 16384 characters per database row and 64 KiB total background. System/tool/internal messages are excluded. Truncation is recorded. Background is quoted data in the initial task, never a change to the frozen system prompt or a recurring injection.

Executions renew a heartbeat every 15 seconds. After five minutes without renewal, list/status/send lazily settles an expired child as interrupted, preserving history, model selection and tree identity. Recovery is idempotent and fences late writes. It never replays tools automatically. Send a new task (or resume with a message) to continue in the same session with a new run ID; before expiry, wait for settlement.
