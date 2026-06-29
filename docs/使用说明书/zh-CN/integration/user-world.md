﻿---
title: 用户世界接口
summary: `/wunder/user_world/*` 是 Wunder 里的用户到用户通信域，与用户到智能体会话域并行存在。
read_when:
  - 用户要接用户之间的单聊或群聊
  - 用户想分清 user_world 和 chat 两套接口的边界
source_docs:
  - docs/API文档.md
  - docs/设计文档/01-系统总体设计.md
  - src/api/user_world.rs
  - src/api/user_world_ws.rs
---

# 用户世界接口

Wunder 不只有“用户和智能体聊天”这一种通信。

它还单独维护了一套“用户和用户通信域”，也就是：

- `/wunder/user_world/*`

## 接口解决的问题

它解决的是这些场景：

- 联系人列表
- 用户间单聊
- 群聊
- 已读状态
- 用户世界消息事件流

也就是说，`user_world` 是并行的另一套消息域。

## 与 `/wunder/chat/*` 的区别

最核心的区别：

- `/wunder/chat/*` 面向用户和智能体
- `/wunder/user_world/*` 面向用户和用户

如果用户把这两个域混在一起理解，前端状态、会话模型和鉴权都会变乱。

## 用户通常会用到哪些接口

### 联系人与群聊

- `GET /wunder/user_world/contacts`
- `GET /wunder/user_world/groups`
- `POST /wunder/user_world/groups`

### 会话

- `POST /wunder/user_world/conversations`
- `GET /wunder/user_world/conversations`
- `GET /wunder/user_world/conversations/{conversation_id}`

### 消息

- `GET /wunder/user_world/conversations/{conversation_id}/messages`
- `POST /wunder/user_world/conversations/{conversation_id}/messages`
- `POST /wunder/user_world/conversations/{conversation_id}/read`

### 实时流

- `GET /wunder/user_world/conversations/{conversation_id}/events`
- `GET /wunder/user_world/ws`

## 优先推荐 WebSocket 的原因

和主聊天域一样，用户世界也优先 WebSocket。

原因很简单：

- 联系人、会话和已读变化都更适合实时推送
- 群聊场景下 SSE 只适合作为兼容或兜底

所以如果用户在做完整客户端，优先接：

- `/wunder/user_world/ws`

## 事件

当前最核心的事件有两个：

- `uw.message`
- `uw.read`

用户可以把它们理解成：

- 新消息
- 已读更新

这已经足够驱动大多数聊天 UI。

## 群聊与单聊的区分

这套模型本身支持两种会话：

- `direct`
- `group`

群聊对象会额外带：

- `group_id`
- `group_name`
- `member_count`
- `announcement`

因此前端不需要再自己猜“这是不是群聊”。

## 文件与语音的处理

用户世界不是只能传纯文本。

当前还支持：

- 会话内文件下载
- 语音消息

语音场景下，`content_type` 可以是：

- `voice`
- `audio/*`

而 `content` 通常用 JSON 字符串承载 `path`、`duration_ms` 等元数据。

## 不适用场景

如果用户的目标是：

- 给模型发任务
- 管理智能体会话
- 看工具调用和中间过程

那用户应该接的是 `/wunder` 或 `/wunder/chat/*`，不是 `user_world`。

## 典型接入顺序

如果用户在做完整客户端，推荐顺序是：

1. 拉联系人和会话列表
2. 进入会话后拉历史消息
3. 建立 `/wunder/user_world/ws`
4. 收到 `uw.message` 和 `uw.read` 后增量更新界面

## 延伸阅读

- [用户侧前端](/docs/zh-CN/surfaces/frontend/)
- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
- [外部登录与免登嵌入](/docs/zh-CN/integration/external-login/)
