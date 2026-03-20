---
title: 聊天 WebSocket
summary: `/wunder/chat/ws` 是 Wunder 聊天主实时通道；实时会话优先 WS，SSE 作为兜底。
read_when:
  - 你在开发聊天 UI 或桌面实时会话
  - 你要处理 start/resume/watch/cancel/approval
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
  - src/api/chat_ws.rs
---

# 聊天 WebSocket

`/wunder/chat/ws` 不只是“事件推流”，它同时是会话控制通道。

## 适用场景

- 聊天工作台或桌面会话
- 同连接内执行 `start / resume / watch / cancel`
- 需要长连接保活与实时状态一致性

## 端点

- `GET /wunder/chat/ws`

## 连接后可做的动作

- `connect`：握手与协议协商
- `start`：启动一次执行
- `resume`：断线后续传事件
- `watch`：只观察某个会话
- `cancel`：取消当前执行
- `approval`：回传审批决策
- `ping`：保活

服务端关键回包：`ready`、`event`、`error`、`pong`

## 最短接入顺序

1. `POST /wunder/chat/sessions` 拿到 `session_id`
2. 建立 `WebSocket /wunder/chat/ws`
3. 发送 `connect`
4. 收到 `ready`
5. 发送 `start`
6. 消费后续 `event`，直至 `turn_terminal`

## 最小握手示例

```json
{
  "kind": "connect",
  "request_id": "req_connect_01",
  "payload": {
    "protocol_version": "1.0",
    "client_name": "my-chat-ui"
  }
}
```

## 启动一次执行

```json
{
  "kind": "start",
  "request_id": "req_start_01",
  "session_id": "sess_xxx",
  "payload": {
    "content": "继续帮我整理刚才那份周报",
    "stream": true
  }
}
```

## 客户端建议重点消费的事件

- `queued`
- `approval_resolved`
- `error`
- `turn_terminal`

`turn_terminal` 是单轮执行的终态信号，建议作为前端收敛条件。

## 常见误区

- `start` 必须显式传 `session_id`。
- 文本字段是 `content`，不是 `/wunder` 的 `question`。
- `resume`（补事件）和 `watch`（观察会话）不是同一个动作。
- WS 审批只处理 `source=chat_ws` 的审批项。

## 失败回退

WS 不可用时：

- 回退 `POST /wunder`（SSE）
- 或回退 `GET /wunder/chat/sessions/{session_id}/resume`

## 延伸阅读

- [聊天会话](/docs/zh-CN/integration/chat-sessions/)
- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [流式事件参考](/docs/zh-CN/reference/stream-events/)
