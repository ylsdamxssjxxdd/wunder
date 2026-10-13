---
title: 用户世界接口
summary: 用户和用户之间的单聊、群聊、已读状态都在这里，和用户对智能体的会话是两套并行的域。
---

# 用户世界接口

wunder 不只有“用户和智能体聊天”。它还单独有一套“用户和用户通信域”：

- `/wunder/user_world/*`

## 适用范围

- 目标是给模型发任务、管理智能体会话、看工具调用 → 用 [wunder API](/docs/zh-CN/integration/wunder-api/) 或 [聊天会话](/docs/zh-CN/integration/chat-sessions/)，不是这里。
- 目标是做联系人、用户间单聊、群聊、已读 → 用这里。

一句话分界：`/wunder/chat/*` 面向用户和智能体，`/wunder/user_world/*` 面向用户和用户。两套别混，混了前端状态和鉴权都会乱。

## 最短链路：接一个完整客户端

1. 拉联系人和会话列表；
2. 进会话后拉历史消息；
3. 建立 `/wunder/user_world/ws`；
4. 收到新消息和已读更新后，增量刷新界面。

## 常用接口

联系人与群聊：

- `GET /wunder/user_world/contacts`
- `GET /wunder/user_world/groups`
- `POST /wunder/user_world/groups`

会话：

- `POST /wunder/user_world/conversations`
- `GET /wunder/user_world/conversations`
- `GET /wunder/user_world/conversations/{conversation_id}`

消息：

- `GET /wunder/user_world/conversations/{conversation_id}/messages`
- `POST /wunder/user_world/conversations/{conversation_id}/messages`
- `POST /wunder/user_world/conversations/{conversation_id}/read`

实时流：

- `GET /wunder/user_world/conversations/{conversation_id}/events`
- `GET /wunder/user_world/ws`

## 关键设置与常见坑

- 和主聊天域一样，优先用 WebSocket：联系人、会话、已读这些变化更适合实时推送，SSE 只适合当兼容或兜底。
- 核心事件就两类：`uw.message`（新消息）和 `uw.read`（已读更新），驱动大多数聊天界面已经够用。
- 会话分 `direct`（单聊）和 `group`（群聊）；群聊会额外带上群 ID、群名、成员数、群公告，前端不用自己猜。
- 不只是纯文本：会话内支持文件下载和语音消息。语音用 `voice` 或 `audio/*` 表示类型，内容里用 JSON 承载路径、时长等元信息。

## 延伸阅读

- [用户侧前端](/docs/zh-CN/surfaces/frontend/)
- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
- [外部登录与免登嵌入](/docs/zh-CN/integration/external-login/)
