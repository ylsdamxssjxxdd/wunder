---
title: Memory Manager
summary: The seven actions of `memory_manager`, the short-index-injection plus fetch-on-demand usage pattern, a write example, and field constraints.
updated_at: 2026-10-16
---

`memory_manager` manages the current agent's long-term memory within the current user scope, with seven supported actions: `list`, `search`, `get`, `add`, `update`, `remove`, `clear`. Each action has a closed field branch: list/search reject write fields, `get` is the only full-content read, and writes/deletes return only an id and status.

## Injection and retrieval

- By default the system prompt receives only a short index (memory id plus title); the full content is **not** injected along with it.
- When detail is needed, first use `list` / `search` to find the memory id, then use `get` to fetch the full text.
- Keep memory ids within 8 characters when possible, so they are easy to search and locate.
- `add` / `update` use only the current core fields: title, content, tag, related memory id, and memory time.

## Actions

- `list`: returns recent memory indexes, 30 by default. Each entry contains id, title, tag, and update time.
- `search`: matches on title or content, 10 by default. A match includes a snippet and whether it matched in the title or the content.
- `get`: reads the full content by id, and also returns the related memory id and memory time.
- `add`: adds one entry, given title, content, tag, and memory time together.
- `update`: updates by id.
- `remove`: removes by id.
- `clear`: clears everything.

Write example:

```json
{
  "action": "add",
  "title": "User name",
  "content": "The user's name is Zhou Huajian. This came from the user's self-introduction at the start of the conversation.",
  "tag": "profile",
  "memory_time": "2026-04-12T08:37:00+08:00"
}
```

## Boundaries

- The old `summary`, `tags`, `entities`, and `category` are no longer recommended inputs.
- `list` / `search` are the main retrieval actions, and `get` is the only action that reads full detail.
- Writes only affect later new threads; they do not write back into the system prompt of a thread that is already frozen.
