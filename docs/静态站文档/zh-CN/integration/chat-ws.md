---
title: 聊天 WebSocket
summary: `/wunder/chat/ws` 是 Wunder 对话态的主实时通道，支持 start、resume、watch、cancel 与审批回传。
read_when:
  - 你要做聊天界面或桌面实时会话
  - 你要优先使用 WebSocket，而不是只接 SSE
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
  - src/api/chat_ws.rs
---

# 聊天 WebSocket

Wunder 的实时对话链路默认优先走 WebSocket，SSE 作为兜底。

如果你在做：

- 聊天页
- 桌面端会话
- 需要 resume/watch/cancel 的长任务面板

那你应该优先接 `/wunder/chat/ws`。

## 端点

- `GET /wunder/chat/ws`

## 为什么这条链路比直接 SSE 更适合聊天

它不只是把事件推给你，还把“会话控制”也放进了同一条连接：

- 启动会话执行
- 续传历史事件
- 只观察某个会话
- 取消当前执行
- 回传审批决策
- 心跳保活

这比纯 SSE 更适合消息工作台。

## 典型接入顺序

1. 先通过 `/wunder/chat/sessions` 创建或拿到 `session_id`
2. 建立 `WebSocket /wunder/chat/ws`
3. 发送 `connect`
4. 收到 `ready`
5. 发送 `start`
6. 处理后续 `event / error / pong`

## 客户端消息类型

- `connect`
- `start`
- `resume`
- `watch`
- `cancel`
- `approval`
- `ping`

## 服务端消息类型

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

这里有两个关键点：

- `start` 需要明确 `session_id`
- payload 里的文本字段是 `content`，不是 `/wunder` 里的 `question`

## 续传与观察

### resume

用于“我之前断线了，现在从某个事件之后继续补回”。

最常用字段：

- `session_id`
- `after_event_id`

### watch

用于“我当前只观察某个会话的事件流，不一定要主动 start 一次新执行”。

这在多面板、多会话切换时很有用。

## 取消与审批

### cancel

可以取消当前 request，也可以按 `session_id` 取消会话中的正在执行任务。

### approval

当工具链进入审批等待态时，前端可以通过 WebSocket 把审批结果回传。

当前实现里，WebSocket 只会处理 `source=chat_ws` 的审批项，不会误操作渠道侧审批。

## 建议你重点消费的事件

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

- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
