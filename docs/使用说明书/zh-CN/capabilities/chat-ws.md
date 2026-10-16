---
title: 聊天 WebSocket
summary: `/wunder/chat/ws` 的握手流程、客户端消息类型、服务端返回与断线重连方式。
updated_at: 2026-10-16
---

# 聊天 WebSocket

`/wunder/chat/ws` 既是事件推送通道，也是会话的控制通道：启动执行、断线续传、观察、取消、回传审批都在同一条连接上完成。聊天与桌面实时会话都用它，没有 SSE 兜底。

## 适用场景

- 聊天工作台与桌面会话；
- 在同一条连接里启动、续传、观察、取消一次执行；
- 长连接保活与一致的实时状态。

## 握手流程

1. 创建会话，拿到会话 ID；
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

## 消息类型

客户端发往服务端：

| kind | 作用 |
| --- | --- |
| `connect` | 握手与协议协商 |
| `start` | 启动一次执行 |
| `resume` | 断线后补齐落下的事件 |
| `watch` | 只观察某个会话 |
| `cancel` | 取消当前执行 |
| `approval` | 回传审批决定 |
| `ping` | 保活 |

服务端主要回 `ready`、`event`、`error`、`pong`。

## 设置与限制

- 需要消费的事件类别：排队中、审批已处理、出错、本轮结束。`turn_terminal` 是单轮执行的终态信号，适合作为前端收敛条件。
- `start` 必须显式带上会话 ID。
- 正文用 `content`，不是 `/wunder` 的 `question`。
- `resume`（补事件）与 `watch`（看运行态）是两个不同动作。
- 这条连接上的审批只处理来自 WebSocket 的审批项。

## 断线与重连

不要退回 SSE。重新连上 `/wunder/chat/ws`，再按场景发送：

- `resume`：带上最后处理到的事件位置，补齐断线期间的事件；
- `watch`：重新观察当前会话的运行态。
