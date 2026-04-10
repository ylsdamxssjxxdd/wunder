---
title: 渠道工具
summary: `channel_tool` 的联系人查询、发信与同步/异步回执结构。
read_when:
  - 你要列外部渠道联系人或发送渠道消息
source_docs:
  - src/services/tools/channel_tool.rs
updated_at: 2026-04-10
---

# 渠道工具

`channel_tool` 主要有两个动作：

- `list_contacts`
- `send_message`

## `list_contacts`

### 最小参数

```json
{
  "action": "list_contacts",
  "channel": "xmpp",
  "account_id": "acc_xxx",
  "keyword": "alice"
}
```

### 成功返回

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

### 最小参数

```json
{
  "action": "send_message",
  "to": "alice@example.com",
  "peer_kind": "user",
  "content": "你好",
  "wait": true
}
```

### `wait=true` 时

更接近同步投递完成：

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

### `wait=false` 时

更接近已入发件箱：

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

## 重点

- 正文统一用 `content`
- `wait` 决定你拿到的是“已送达结果”还是“已排队结果”
