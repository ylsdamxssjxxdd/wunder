---
title: 聊天 WebSocket
summary: `/wunder/chat/ws` 是 Wunder 聊天域的主实时通道；实时会话优先走它，SSE 只做兜底。
read_when:
  - 你要做聊天界面或桌面实时会话
  - 你要优先使用 WebSocket，而不是只接 SSE
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
  - src/api/chat_ws.rs
---

# 聊天 WebSocket

如果你在做实时聊天 UI，这页比 SSE 更该先看。

Wunder 的实时对话链路默认优先走 WebSocket，SSE 作为兜底。

## 这页解决什么

- 如何接入 `/wunder/chat/ws`
- 为什么聊天产品优先走 WebSocket
- start、resume、watch、cancel 和审批回传怎么理解

## 先记这条

- WebSocket 不只是事件流，它同时承担会话控制。
- 你可以在同一条连接里 start、resume、watch、cancel 和 approval。
- 这比“一个请求 + 一条 SSE”更适合聊天工作台。

## 端点

- `GET /wunder/chat/ws`

## 什么时候优先接 WebSocket

- 你要做聊天页或桌面端会话
- 你要切换会话后继续 watch 某个 session
- 你要在一条连接里做启动、取消、恢复和审批
- 你要长连接保活和更完整的运行态控制

## 为什么它比直接 SSE 更适合聊天

它不只是把事件推给你，还把“会话控制”也放进了同一条连接：

- 启动会话执行
- 续传历史事件
- 只观察某个会话
- 取消当前执行
- 回传审批决策
- 心跳保活

## 最短接入顺序

1. 先通过 `/wunder/chat/sessions` 创建或拿到 `session_id`
2. 建立 `WebSocket /wunder/chat/ws`
3. 发送 `connect`
4. 收到 `ready`
5. 发送 `start`
6. 处理后续 `event / error / pong`

## 客户端会发什么

- `connect`
- `start`
- `resume`
- `watch`
- `cancel`
- `approval`
- `ping`

## 服务端会回什么

- `ready`
- `event`
- `error`
- `pong`

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

收到 `ready` 后，可以开始发送业务消息。

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

## 最容易搞错的点

- `start` 需要明确 `session_id`。
- payload 里的文本字段是 `content`，不是 `/wunder` 里的 `question`。
- `resume` 是断线补事件，`watch` 是只观察会话，不要把两者混成一个动作。
- 当前实现里，WebSocket 只处理 `source=chat_ws` 的审批项，不会误操作渠道侧审批。

## 建议重点消费这些事件

虽然 `event.data` 里会有很多业务事件，但对实时会话面板来说，这几个最关键：

- `queued`
- `approval_resolved`
- `error`
- `turn_terminal`

尤其是 `turn_terminal`，它是单轮执行的最终终态信号。

## 失败回退

如果 WebSocket 不可用：

- 聊天链路可以回退到 `/wunder` SSE
- 或回退到 `/wunder/chat/sessions/{session_id}/resume`

Wunder 的设计不是“只有 WS 能用”，而是“WS 优先，SSE 保底”。

## 相关文档

- [聊天会话](/docs/zh-CN/integration/chat-sessions/)
- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
