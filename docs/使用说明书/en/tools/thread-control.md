---
title: Thread Control
summary: The actions, thread relationships, and return structure of `thread_control`.
read_when:
  - You need to create, switch, archive, restore, or assign the main thread
source_docs:
  - src/services/tools/thread_control_tool.rs
updated_at: 2026-04-10
---

# Thread Control

`thread_control` manages conversation threads between the current user and the current agent. It does not manage child-agent runs.

## Main actions

- `list`
- `info`
- `create`
- `switch`
- `back`
- `update_title`
- `archive`
- `restore`
- `set_main`

## Return envelope

Successful calls share this structure:

```json
{
  "ok": true,
  "action": "create",
  "state": "completed",
  "summary": "Created a new thread.",
  "data": { ... }
}
```

Some actions also include `next_step_hint`.

## `list`

Typical result:

```json
{
  "ok": true,
  "action": "list",
  "state": "completed",
  "summary": "Listed 5 threads.",
  "data": {
    "scope": "branch",
    "status": "active",
    "items": [
      {
        "id": "sess_xxx",
        "title": "Tool documentation update",
        "status": "active",
        "created_at": "2026-04-10T10:00:00+08:00",
        "updated_at": "2026-04-10T10:03:00+08:00",
        "last_message_at": "2026-04-10T10:03:00+08:00",
        "agent_id": "agent_docs",
        "parent_session_id": null,
        "spawn_label": null,
        "is_main": true,
        "runtime_status": "idle"
      }
    ]
  }
}
```

## `create`

The key point is that creation can also be combined with:

- `switch`
- `set_main`

That means you can create a thread and immediately switch to it or mark it as the main thread.

## `set_main`

This is an important action. It binds a given conversation thread as the current agent's first-class main thread.

## Difference from `subagent_control`

- `thread_control`: manages the conversation threads themselves
- `subagent_control`: manages temporary child-agent runs

If you want to manage main threads, branch threads, or archive status, use `thread_control`.  
If you need to wait for a subagent, interrupt it, or inspect its history, use `subagent_control`.
