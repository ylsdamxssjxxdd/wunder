---
title: wunder API
summary: 把 wunder 当成一个执行能力来调用，一次请求出一次结果；要做完整聊天产品再转到会话域。
---

# wunder API

`POST /wunder` 适合“把 wunder 当执行能力调用”。要做会话列表、恢复、取消、实时工作台，直接去聊天域。

## /wunder 与聊天域的分工

- 一次请求触发一次执行、只关心结果 → 用 `/wunder`。
- 要历史、恢复、取消、稳定绑定智能体、读取冻结提示词 → 用 [聊天会话](/docs/zh-CN/integration/chat-sessions/)。

## 最短链路：稳定绑定智能体

`GET /wunder/agents` → `POST /wunder/chat/sessions` → `POST /wunder/chat/sessions/{session_id}/messages`

1. 先拿到目标智能体 ID；
2. 创建会话并绑定这个智能体；
3. 发送消息；
4. 用 WebSocket 或续传接口消费流式事件。

## 最小请求

```json
{
  "user_id": "demo_user",
  "question": "帮我整理今天的工作计划",
  "stream": true
}
```

常用补充：

- `session_id`：复用会话
- `agent_id`：指定智能体
- `model_name`：临时换模型
- `tool_names`：显式挂工具
- `config_overrides`：局部覆盖配置
- `attachments`：附件输入

## 附件别一股脑丢进来

官方前端对附件是分流的：

- 图片：可以直接作为视觉上下文发。
- 文档：先转成文本再提交。
- 音频：先转写成文本附件。
- 视频：先拆成图片序列和音轨，不会把原始视频发给模型。

绕过这些预处理直接调 `/wunder`，拆解、转写、抽帧和体积控制就得你自己负责。

## 关键设置与常见坑

- `user_id` 表示调用者和隔离空间，不必是已注册用户。
- `agent_id` 表示目标智能体。
- `/wunder` 支持指定智能体，但不等于它有聊天域的完整能力；会话治理能力要自行补齐。
- 别把 `user_id` 理解成“必须来自用户管理表”，那会造成没必要的接入限制。

## 相关接口

- `POST /wunder`
- `POST /wunder/system_prompt`
- `GET /wunder/tools`
- `GET /wunder/agents`
- `POST /wunder/chat/sessions`
- `POST /wunder/chat/sessions/{session_id}/messages`

## 延伸阅读

- [聊天会话](/docs/zh-CN/integration/chat-sessions/)
- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
- [API 索引](/docs/zh-CN/reference/api-index/)
