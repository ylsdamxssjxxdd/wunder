---
title: User World
summary: Internal user lookup and in-product message delivery.
read_when:
  - You need to query users inside wunder, or send a message to them
source_docs:
  - src/services/tools.rs
updated_at: 2026-04-10
---

# User World

`user_world` mainly exposes two actions:

- `list_users`
- `send_message`

## `list_users`

```json
{
  "ok": true,
  "action": "list_users",
  "state": "completed",
  "summary": "Listed 20 users from user world.",
  "data": {
    "items": [
      {
        "user_id": "user_xxx",
        "username": "alice",
        "status": "active",
        "unit_id": "unit_xxx"
      }
    ],
    "total": 20,
    "offset": 0,
    "limit": 20
  }
}
```

## `send_message`

```json
{
  "ok": true,
  "action": "send_message",
  "state": "completed",
  "summary": "Processed 2 user world message deliveries.",
  "data": {
    "results": [
      {
        "user_id": "user_xxx",
        "status": "sent"
      }
    ],
    "staged_files": []
  }
}
```

## Key points

- This is the in-product user world, not an external channel
- If attachments were staged for delivery, they appear in `staged_files`
