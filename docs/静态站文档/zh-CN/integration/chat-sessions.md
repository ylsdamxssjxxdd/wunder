---
title: 聊天会话
summary: `/wunder/chat/sessions/*` 是 Wunder 的聊天主域，不只是“会话列表接口”。
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

- 如何创建、列出和切换一个聊天会话
- 如何向指定 `session_id` 发送消息、续传历史和取消执行
- 如何获取会话级 `runtime`、事件和系统提示词快照

## 先看这些接口

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
- `POST /wunder/chat/sessions/{session_id}/archive`
- `POST /wunder/chat/sessions/{session_id}/restore`
- `POST /wunder/chat/sessions/{session_id}/title`
- `POST /wunder/chat/sessions/{session_id}/tools`

## 什么时候优先看这组接口

- 你要做会话列表、详情页和历史分页
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

## 最容易搞错的点

- 消息接口的文本字段是 `content`，不是 `/wunder` 那套 `question`。
- 会话状态优先看 `runtime`，不要只看兼容字段 `running`。
- 当前线程的 system prompt 预览可能是 `pending`，也可能已经 `frozen`。

## 为什么还要看提示词预览

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

## 相关文档

- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [运行时与在线状态](/docs/zh-CN/concepts/presence-and-runtime/)
