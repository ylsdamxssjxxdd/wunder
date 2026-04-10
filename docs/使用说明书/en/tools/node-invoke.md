---
title: Node Invoke
summary: Gateway node listing and command invocation.
read_when:
  - You need to list available nodes or send a command to a specific node
source_docs:
  - src/services/tools.rs
updated_at: 2026-04-10
---

# Node Invoke

`node_invoke` mainly exposes two actions:

- `list`
- `invoke`

## `list`

```json
{
  "ok": true,
  "action": "list",
  "state": "completed",
  "summary": "Listed 6 gateway nodes.",
  "data": {
    "state_version": 42,
    "count": 6,
    "nodes": [ ... ]
  }
}
```

## `invoke`

```json
{
  "ok": true,
  "action": "invoke",
  "state": "completed",
  "summary": "Invoked command ping on node node_a.",
  "data": {
    "node_id": "node_a",
    "command": "ping",
    "result": { ... }
  }
}
```

## Key points

- Call `list` before `invoke`
- The real command output lives in `data.result`
