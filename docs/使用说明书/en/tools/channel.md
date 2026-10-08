---
title: Channel Tool
summary: The contact lookup, message sending, and synchronous or asynchronous receipt structures of `channel_tool`.
read_when:
  - You need to list external channel contacts or send a channel message
source_docs:
  - src/services/tools/channel_tool.rs
updated_at: 2026-04-10
---

# Channel Tool

`channel_tool` mainly has two actions:

- `list_contacts`
- `send_message`

## `list_contacts`

### Minimum arguments

```json
{
  "action": "list_contacts",
  "channel": "xmpp",
  "account_id": "acc_xxx",
  "keyword": "alice"
}
```

### Success result

```json
{
  "ok": true,
  "action": "list_contacts",
  "state": "completed",
  "summary": "Listed 12 contacts.",
  "data": {
    "action": "list_contacts",
    "items": [ ... ],
    "total": 12,
    "offset": 0,
    "limit": 20,
    "warnings": [],
    "resolved_scope": {
      "source": "session",
      "channel": "xmpp",
      "account_id": "acc_xxx"
    }
  }
}
```

## `send_message`

### Minimum arguments

```json
{
  "action": "send_message",
  "to": "alice@example.com",
  "peer_kind": "user",
  "content": "Hello",
  "wait": true
}
```

### With `wait=true`

This is closer to a synchronous delivery result:

```json
{
  "ok": true,
  "action": "send_message",
  "state": "completed",
  "summary": "Sent channel message.",
  "data": {
    "delivery": { ... },
    "resolved": { ... }
  }
}
```

### With `wait=false`

This is closer to "queued in outbox":

```json
{
  "ok": true,
  "action": "send_message",
  "state": "accepted",
  "summary": "Queued channel message for delivery.",
  "data": {
    "outbox_id": "out_xxx",
    "status": "pending",
    "resolved": { ... }
  },
  "next_step_hint": "..."
}
```

## Key points

- The message body is always `content`
- `wait` determines whether you receive a final delivery result or only a queued-delivery result
