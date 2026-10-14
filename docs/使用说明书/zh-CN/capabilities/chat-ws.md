---
title: 聊天 WebSocket
summary: 聊天的实时通道，同时也是会话控制通道：启动执行、断线续传、观察、取消、回传审批都在同一条连接上。
---

# 聊天 WebSocket

`/wunder/chat/ws` 不只是把事件推给你，它同时是会话的控制通道。聊天和桌面实时会话都用它，不再做 SSE 兜底。

## 适用场景

- 做聊天工作台或桌面会话；
- 要在同一条连接里启动、续传、观察、取消一次执行；
- 需要长连接保活和一致的实时状态。

## 最短链路：几步接通

1. 先创建会话，拿到会话 ID；
2. 建立 WebSocket 连接；
3. 发送 `connect` 握手；
4. 收到 `ready`；
5. 发送 `start` 启动执行；
6. 持续消费 `event`，直到收到本轮终态 `turn_terminal`。

握手示例：

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

启动一次执行：

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

## 连接上能做什么

- `connect`：握手与协议协商
- `start`：启动一次执行
- `resume`：断线后把落下的补回来
- `watch`：只观察某个会话
- `cancel`：取消当前执行
- `approval`：回传审批决定
- `ping`：保活

服务端主要回 `ready`、`event`、`error`、`pong`。

## 关键设置与常见坑

- 重点消费这几类事件：排队中、审批已处理、出错、本轮结束。`turn_terminal` 是单轮执行的终态信号，适合作为前端收敛条件。
- `start` 必须显式带上会话 ID。
- 正文用 `content`，不是 `/wunder` 的 `question`。
- `resume`（补事件）和 `watch`（看运行态）是两个不同动作。
- 这条连接上的审批只处理来自 WebSocket 的审批项。

## 断线了怎么办

不要退回 SSE。重新连上 `/wunder/chat/ws`，再按场景发送：

- `resume`：带上最后处理到的事件位置，补齐断线期间的事件；
- `watch`：重新观察当前会话的运行态。

## 延伸阅读

- [流式事件参考](/docs/zh-CN/capabilities/stream-events/)
