---
title: 聊天会话
summary: 做聊天产品时用的会话接口：创建会话、收发消息、附件预处理、恢复与取消，都在这个域里。
---

# 聊天会话

要做的是聊天产品，而不只是偶尔调一次能力，就把会话接口作为主域。

## 会话域的适用条件

- 只要一次执行、不要历史 → 用 [wunder API](/docs/zh-CN/integration/wunder-api/) 就够。
- 要会话列表、历史、恢复、取消、实时工作台 → 用会话域。

## 最短链路：接通一个会话

1. 创建会话，拿到会话 ID；
2. 文档附件先做一次文本转换；
3. 音频、视频附件先做一次媒体处理；
4. 发送正文和（或）附件；
5. 用 [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/) 的 start / resume / watch 消费实时事件；
6. 拉会话详情渲染界面；
7. 需要时取消或压缩会话。

## 主要接口

会话：

- `GET/POST /wunder/chat/sessions`
- `GET/DELETE /wunder/chat/sessions/{session_id}`
- `POST /wunder/chat/sessions/{session_id}/messages`
- `GET /wunder/chat/sessions/{session_id}/events`
- `POST /wunder/chat/sessions/{session_id}/cancel`
- `POST /wunder/chat/sessions/{session_id}/compaction`
- `POST /wunder/chat/sessions/{session_id}/system-prompt`

附件：

- `POST /wunder/chat/attachments/convert`
- `POST /wunder/chat/attachments/media/process`

## 附件怎么走

- 图片：直接作为附件发，走视觉上下文。
- 文档：先转成文本型附件再发。
- 音频：先做媒体处理拿到转写结果，再提交消息。
- 视频：同样先做媒体处理，服务端会把它拆成图片序列和音轨附件——原始视频不会直接送进模型。

消息允许“只有附件、没有正文”：只要附件里有有效内容或可访问路径，就可以不传文本。

## 关键设置与常见坑

- 提示词预览：会话域能返回提示词状态和记忆预览，让前端明确告诉用户当前是“待定”还是“已冻结”。
- 消息正文字段是 `content`，不是 `/wunder` 那套的 `question`。
- 会话状态以运行时字段为准，别只看为兼容保留的“运行中”标记。
- 会话和线程不是同一层语义，界面设计时分开处理。
- 聊天输入区帮你自动做的附件预处理，自己接接口时也要在发消息前补上。

## 延伸阅读

- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [运行时与在线状态](/docs/zh-CN/concepts/presence-and-runtime/)
