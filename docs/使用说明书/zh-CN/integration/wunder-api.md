---
title: "wunder API"
summary: "`/wunder` 是统一执行入口，适合能力调用；当你要做完整聊天产品时，优先切到 `/wunder/chat/*`。"
read_when:
  - "你要从业务系统接入 Wunder"
  - "你要判断该走 `/wunder` 还是 `/wunder/chat/*`"
source_docs:
  - "docs/API文档.md"
  - "docs/系统介绍.md"
  - "docs/设计方案.md"
---

# wunder API

`POST /wunder` 适合“把 Wunder 当执行能力调用”。

如果你要做会话列表、恢复、取消、实时工作台，建议直接进入聊天域：`/wunder/chat/*`。

## 什么时候优先用 `/wunder`

- 一次请求触发一次执行
- 调用侧只需要 `user_id + question` 语义
- 需要快速接入并支持流式输出（SSE）

## 什么时候不要只用 `/wunder`

- 你要完整会话生命周期管理
- 你要稳定绑定智能体并读取冻结提示词
- 你要做聊天产品级 UI（历史、恢复、取消、观察）

## 推荐接入链路（稳定绑定智能体）

`GET /wunder/agents` -> `POST /wunder/chat/sessions` -> `POST /wunder/chat/sessions/{session_id}/messages`

1. 用 `GET /wunder/agents` 获取目标 `agent_id`
2. 用 `POST /wunder/chat/sessions` 创建会话并绑定智能体
3. 用 `POST /wunder/chat/sessions/{session_id}/messages` 发送消息
4. 用 WS 或 `resume` 消费流式事件

## 请求字段（最小集合）

```json
{
  "user_id": "demo_user",
  "question": "帮我整理今天的工作计划",
  "stream": true
}
```

常用补充字段：

- `session_id`：复用会话
- `agent_id`：指定智能体范围
- `model_name`：临时覆盖模型
- `tool_names`：显式挂载工具
- `config_overrides`：局部配置覆盖
- `attachments`：附件输入（图片可直接传；文档、音频、视频建议先做预处理）

## 附件不要一股脑直接丢给 `/wunder`

官方前端现在对附件的处理是分流的：

- 图片：可直接作为视觉上下文进入 `attachments`
- 文档：通常先走 `/wunder/chat/attachments/convert` 或 `/wunder/doc2md/convert`，转成文本再提交
- 音频：先走 `/wunder/chat/attachments/media/process`，把语音转成文本附件
- 视频：先走 `/wunder/chat/attachments/media/process`，拆成图片序列和音轨附件；不是直接把原始视频发给模型

如果你绕过这些预处理，自己直接调 `/wunder`，那附件拆解、转写、抽帧和体积控制都要由接入方自己负责。

## 身份字段说明

- `user_id` 表示调用者与隔离空间，不要求一定是已注册用户。
- `agent_id` 表示目标智能体。

## 常见误区

- `/wunder` 支持 `agent_id`，不代表它等价于聊天域的完整能力。
- 如果直接调 `/wunder`，很多会话治理能力要你自行补齐。
- 把 `user_id` 误解为“必须来自用户管理表”，会导致不必要的接入限制。

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
