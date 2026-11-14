---
title: 聊天 WebSocket
summary: `/wunder/chat/ws` 的握手流程、客户端消息类型、服务端返回与断线重连方式。
updated_at: 2026-11-14
---

# 聊天 WebSocket

`/wunder/chat/ws` 既是事件推送通道，也是控制通道：启动执行、断线续传、观察、取消、回传审批都在同一条连接上完成。聊天与桌面实时会话共用它。

## 握手

1. 创建会话，拿到会话 ID；
2. 建立 WebSocket 连接；
3. 发 `connect` 完成握手；
4. 收到 `ready`；
5. 发 `start` 启动执行；
6. 持续消费 `event`，直到本轮终态。

```json
{
  "kind": "connect",
  "request_id": "req_connect_01",
  "payload": { "protocol_version": "1.0", "client_name": "my-chat-ui" }
}
```

```json
{
  "kind": "start",
  "request_id": "req_start_01",
  "session_id": "sess_xxx",
  "payload": { "content": "继续整理刚才那份周报", "stream": true }
}
```

## 消息类型

| kind | 作用 |
| --- | --- |
| `connect` | 握手与协议协商 |
| `start` | 启动一次执行 |
| `resume` | 断线后补齐落下的事件 |
| `watch` | 只观察某个会话 |
| `cancel` | 取消当前执行 |
| `approval` | 回传审批决定 |
| `ping` | 保活 |

服务端回 `ready`、`event`、`error`、`pong`。其中 `turn_terminal` 是单轮执行的终态信号，适合当前端收敛条件。

## 约定与限制

- `start` 必须显式带会话 ID；正文用 `content`，不是 HTTP 接口的 `question`。
- `resume`（补事件）与 `watch`（看运行态）是两个不同动作，别互相代替。
- 这条连接上的审批只处理经它下发的审批项。
- 断线后重新连同一个地址，再按场景发 `resume` 或 `watch`；桌面版断线由前端自动重连，不需要用户干预。
