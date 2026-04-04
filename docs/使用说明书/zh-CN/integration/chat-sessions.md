---
title: 聊天会话
summary: `/wunder/chat/sessions/*` 是 Wunder 聊天主域，负责会话生命周期、消息收发、附件预处理、事件恢复和运行态管理。
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
- `POST /wunder/chat/attachments/convert`
- `POST /wunder/chat/attachments/media/process`
- `POST /wunder/chat/sessions/{session_id}/messages`
- `GET /wunder/chat/sessions/{session_id}/events`
- `GET /wunder/chat/sessions/{session_id}/resume`
- `POST /wunder/chat/sessions/{session_id}/cancel`
- `POST /wunder/chat/sessions/{session_id}/compaction`
- `POST /wunder/chat/sessions/{session_id}/system-prompt`

## 典型链路

1. `POST /wunder/chat/sessions` 创建会话
2. 文档附件先 `POST /wunder/chat/attachments/convert`
3. 音频 / 视频附件先 `POST /wunder/chat/attachments/media/process`
4. `POST /wunder/chat/sessions/{session_id}/messages` 发送正文和 / 或附件
5. 用 [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/) 或 `resume` 消费事件
6. 用 `GET /wunder/chat/sessions/{session_id}` 渲染会话详情
7. 需要时调用 `cancel` 或 `compaction`

## 附件推荐流程

- 图片：可直接作为 `attachments` 发送，走视觉上下文。
- 文档：先走 `/wunder/chat/attachments/convert`，把文档转成文本型附件。
- 音频：先走 `/wunder/chat/attachments/media/process`，拿到转写结果后再提交消息。
- 视频：也走 `/wunder/chat/attachments/media/process`，服务端会先拆成图片序列和音轨附件；原始视频不会直接送进模型。

消息提交允许“只有附件，没有正文”。只要 `attachments[]` 里存在有效 `content` 或 `public_path`，就可以不传文本正文。

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
- 聊天输入区自动做的附件预处理，如果你自己接 API，也要在消息发送前补上。

## 延伸阅读

- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [运行时与在线状态](/docs/zh-CN/concepts/presence-and-runtime/)
