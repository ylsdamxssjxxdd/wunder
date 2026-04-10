---
title: 用户世界工具
summary: 站内用户查询与站内消息投递。
read_when:
  - 你要查站内用户，或给站内用户发送消息
source_docs:
  - src/services/tools.rs
updated_at: 2026-04-10
---

# 用户世界工具

`user_world` 主要两个动作：

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

## 重点

- 这是站内用户世界，不是外部渠道
- 有附件落地时，会在 `staged_files` 里体现
