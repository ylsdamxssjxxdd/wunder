---
title: 聊天会话
summary: `/wunder/chat/sessions/*` 是 Wunder 的聊天域，不只是“会话列表接口”，而是一整套会话控制面。
read_when:
  - 你在做聊天 UI、桌面会话或消息工作台
  - 你想知道什么时候该用聊天域，而不是直接只调 `/wunder`
source_docs:
  - docs/API文档.md
  - src/api/chat.rs
---

# 聊天会话

如果你要做真正的聊天产品，不要只盯着 `/wunder`，应该把 `/wunder/chat/sessions/*` 当成主域。

## 这页解决什么

这页回答三件事：

- 如何创建、列出和切换一个聊天会话
- 如何向指定 `session_id` 发送消息、续传历史和取消执行
- 如何获取会话级 `runtime`、事件和系统提示词快照

## 最常用的接口

- `GET/POST /wunder/chat/sessions`
- `GET/DELETE /wunder/chat/sessions/{session_id}`
- `POST /wunder/chat/sessions/{session_id}/messages`
- `GET /wunder/chat/sessions/{session_id}/events`
- `GET /wunder/chat/sessions/{session_id}/history`
- `GET /wunder/chat/sessions/{session_id}/resume`
- `POST /wunder/chat/sessions/{session_id}/cancel`
- `POST /wunder/chat/sessions/{session_id}/compaction`
- `POST /wunder/chat/system-prompt`
- `POST /wunder/chat/sessions/{session_id}/system-prompt`

另外还有会话治理接口：

- `POST /wunder/chat/sessions/{session_id}/archive`
- `POST /wunder/chat/sessions/{session_id}/restore`
- `POST /wunder/chat/sessions/{session_id}/title`
- `POST /wunder/chat/sessions/{session_id}/tools`

## 什么时候优先看这组接口

- 你要做会话列表、详情页、历史分页
- 你要在同一个用户下管理多条对话
- 你要在聊天 UI 里支持取消、恢复、观察和压缩
- 你要给用户预览当前线程的冻结提示词

## 一条典型链路

1. `POST /wunder/chat/sessions` 创建会话
2. `POST /wunder/chat/sessions/{session_id}/messages` 发送消息
3. 用 [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/) 或 `resume` 消费流式事件
4. 用 `GET /wunder/chat/sessions/{session_id}` 和 `events` 渲染详情页
5. 需要时调用 `cancel`、`compaction`、`system-prompt`

## 它和 `/wunder` 的区别

可以这样记：

- `/wunder`：统一执行入口
- `/wunder/chat/*`：聊天会话控制面

前者适合“把 Wunder 当能力服务调用”，后者适合“把 Wunder 当聊天系统接入”。

## 你最容易忽略的两个点

### 消息接口的文本字段是 `content`

聊天域消息提交不是传 `question`，而是传 `content`。

### 会话状态应该看 `runtime`

会话详情和事件接口都会返回 `runtime`。

这比只看兼容字段 `running` 更稳定，因为它还能表达：

- `loaded`
- `active`
- `streaming`
- `waiting`
- `pending_approval_count`
- `terminal_status`

## 提示词预览为什么重要

聊天域单独提供：

- `/wunder/chat/system-prompt`
- `/wunder/chat/sessions/{session_id}/system-prompt`

它不仅返回 prompt 文本，还会返回：

- `memory_preview`
- `memory_preview_mode`
- `memory_preview_count`
- `memory_preview_total_count`

所以前端可以明确告诉用户：

- 这是新线程待冻结的提示词
- 还是当前线程已经冻结的提示词

## 你最需要记住的点

- 做聊天产品时，聊天会话接口应该是主入口，不是补充接口。
- 会话详情、事件和 WebSocket 应共享同一套运行时语义。
- 当前线程的 system prompt 预览可能是 `pending`，也可能已经 `frozen`。

## 相关文档

- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [运行时与在线状态](/docs/zh-CN/concepts/presence-and-runtime/)
