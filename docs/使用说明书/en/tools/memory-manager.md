---
title: Memory Manager
summary: The actions, memory scopes, and return structure of `memory_manager`.
read_when:
  - You need to add, remove, update, list, or recall long-term memory entries
source_docs:
  - src/services/tools/memory_manager_tool.rs
updated_at: 2026-04-10
---

# Memory Manager

`memory_manager` now fully uses the standard success envelope.  
Its main actions are:

- `list`
- `recall`
- `add`
- `update`
- `delete`
- `clear`

## `list`

```json
{
  "ok": true,
  "action": "list",
  "state": "completed",
  "summary": "Listed 3 memory entries.",
  "data": {
    "count": 3,
    "items": [
      {
        "memory_id": "mem_xxx",
        "title": "User preference",
        "summary": "Prefers concise answers",
        "content": "The user prefers concise answers",
        "category": "tool-note",
        "tags": ["preference"],
        "status": "active",
        "updated_at": 1760000000
      }
    ],
    "agent_id": "agent_docs"
  }
}
```

## `add`

```json
{
  "ok": true,
  "action": "add",
  "state": "completed",
  "summary": "Saved a memory entry.",
  "data": {
    "memory_id": "mem_xxx",
    "saved": true,
    "agent_id": "agent_docs"
  },
  "next_step_hint": "..."
}
```

## `recall`

```json
{
  "ok": true,
  "action": "recall",
  "state": "completed",
  "summary": "Recalled 2 memory entries.",
  "data": {
    "query": "user preference",
    "count": 2,
    "items": [
      {
        "memory_id": "mem_xxx",
        "title": "User preference",
        "summary": "Prefers concise answers",
        "content": "The user prefers concise answers",
        "category": "tool-note",
        "tags": ["preference"],
        "status": "active",
        "updated_at": 1760000000,
        "why": "matched user preference; in title/summary"
      }
    ],
    "agent_id": "agent_docs"
  }
}
```

## Key points

- `agent_id` is injected directly into `data`
- `recall` is primarily for bringing relevant memory into the current context
- some successful calls include `next_step_hint`, often to remind you that the effect only applies to new sessions or only changes recall semantics for the current session
