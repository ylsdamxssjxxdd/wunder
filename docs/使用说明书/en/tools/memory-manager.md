---
title: Memory Manager
summary: Explains the `memory_manager` actions, compact structure, and the "short index injection + get full detail" workflow.
read_when:
  - You need to add, search, read, update, or remove long-term memory fragments
source_docs:
  - src/services/tools/memory_manager_tool.rs
  - src/services/memory_fragments.rs
updated_at: 2026-04-12
---

# Memory Manager

`memory_manager` manages long-term memory for the current agent inside the current user scope.

Supported actions:

- `list`
- `search`
- `get`
- `add`
- `update`
- `remove`
- `clear`

## Usage Rules

- The system prompt only receives compact memory indexes: `memory_id + title`
- Full `content` is not auto-injected into the prompt
- Use `list/search` first, then `get(memory_id)` for the full detail
- Model-facing `memory_id` should stay within 8 characters when possible
- `add/update` should use only the compact field set:
  - `title`
  - `content`
  - `tag`
  - `related_memory_id`
  - `memory_time`

## `list`

Returns recent memory indexes. Default limit is 30.

```json
{
  "action": "list",
  "data": {
    "count": 1,
    "items": [
      {
        "memory_id": "0695f345",
        "title": "User name",
        "tag": "profile",
        "updated_at": 1775957851
      }
    ]
  }
}
```

## `search`

Searches matching titles or content. Default limit is 10.

```json
{
  "action": "search",
  "data": {
    "query": "Zhou Huajian",
    "count": 1,
    "items": [
      {
        "memory_id": "0695f345",
        "title": "User name",
        "tag": "profile",
        "snippet": "The user's name is Zhou Huajian. This came from the user's self-introduction.",
        "matched_in": ["content"],
        "updated_at": 1775957851
      }
    ]
  }
}
```

## `get`

Loads the full detail for one memory.

```json
{
  "action": "get",
  "data": {
    "memory_id": "0695f345",
    "item": {
      "memory_id": "0695f345",
      "title": "User name",
      "content": "The user's name is Zhou Huajian. This came from the user's self-introduction.",
      "tag": "profile",
      "related_memory_id": null,
      "memory_time": 1775957820,
      "updated_at": 1775957851
    }
  }
}
```

## Write Example

```json
{
  "action": "add",
  "title": "User name",
  "content": "The user's name is Zhou Huajian. This came from the user's self-introduction.",
  "tag": "profile",
  "memory_time": "2026-04-12T08:37:00+08:00"
}
```

## Important Notes

These old fields are no longer part of the recommended protocol:

- `summary`
- `tags`
- `entities`
- `category`

Practical rule:

- `list/search` are the main retrieval actions
- `get` is the only full-detail read action
- Writes only affect later new threads, not an already frozen thread system prompt
