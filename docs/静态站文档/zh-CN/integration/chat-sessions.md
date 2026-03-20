---
title: 聊天会话
summary: `/wunder/chat/sessions/*` 是 Wunder 聊天主域，负责会话生命周期、消息收发、事件恢复和运行态管理。
read_when:
  - 你在做聊天 UI、桌面会话或消息工作台
  - 你要从“能力调用”升级到“会话产品化”
source_docs:
  - docs/API文档.md
  - src/api/chat.rs
---

# 聊天会话

当你需要会话级能力时，应把 `/wunder/chat/sessions/*` 作为主域，而不是只调 `/wunder`。

## 这组接口负责什么

- 创建、列出、删除会话
- 发送消息与消费事件
- 恢复、取消、压缩会话
- 读取会话运行态与系统提示词快照

## 高频接口

- `GET/POST /wunder/chat/sessions`
- `GET/DELETE /wunder/chat/sessions/{session_id}`
- `POST /wunder/chat/sessions/{session_id}/messages`
- `GET /wunder/chat/sessions/{session_id}/events`
- `GET /wunder/chat/sessions/{session_id}/resume`
- `POST /wunder/chat/sessions/{session_id}/cancel`
- `POST /wunder/chat/sessions/{session_id}/compaction`
- `POST /wunder/chat/sessions/{session_id}/system-prompt`

## 典型链路

1. `POST /wunder/chat/sessions` 创建会话
2. `POST /wunder/chat/sessions/{session_id}/messages` 发送消息
3. 用 [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/) 或 `resume` 消费事件
4. 用 `GET /wunder/chat/sessions/{session_id}` 渲染会话详情
5. 需要时调用 `cancel` 或 `compaction`

## 与 `/wunder` 的关系

- `/wunder`：统一执行入口
- `/wunder/chat/*`：会话控制与产品化接口

如果你做的是聊天产品，建议以聊天域为主，`/wunder` 作为补充能力入口。

## 提示词预览为什么重要

聊天域提供：

- `/wunder/chat/system-prompt`
- `/wunder/chat/sessions/{session_id}/system-prompt`

可返回 prompt 状态与记忆预览字段，用于前端明确提示：当前是 `pending` 还是 `frozen`。

## 常见误区

- 消息字段是 `content`，不是 `question`。
- 会话状态优先看 `runtime`，不要只看兼容字段 `running`。
- 会话能力与线程能力不是一层语义，UI 设计要分开处理。

## 延伸阅读

- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [运行时与在线状态](/docs/zh-CN/concepts/presence-and-runtime/)
