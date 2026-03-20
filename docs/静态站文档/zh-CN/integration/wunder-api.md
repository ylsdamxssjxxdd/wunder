---
title: "wunder API"
summary: "`/wunder` 是底层执行入口；如果你要稳定绑定某个智能体或做完整聊天产品，优先走 `/wunder/chat/*`。"
read_when:
  - "你要从业务系统接入 Wunder"
  - "你需要判断该走 `/wunder` 还是 `/wunder/chat/*`"
source_docs:
  - "docs/API文档.md"
  - "docs/系统介绍.md"
  - "docs/设计方案.md"
---

# wunder API

如果你只是想把 Wunder 当能力服务接进去，这页先看。

先记一条：`/wunder` 是底层执行入口；如果你要稳定绑定某个智能体人格，或者你在做完整聊天产品，优先走 `/wunder/chat/*`。

## 这页解决什么

- 什么时候直接调 `/wunder`
- 什么时候不要先调 `/wunder`
- 一条更稳的智能体接入链路是什么

## 先做这个判断

- 你要“把 Wunder 当能力服务调用”，可以先看 `/wunder`
- 你要“把 Wunder 当聊天系统接入”，先看 `/wunder/chat/*`
- 你要稳定绑定某个独立人格智能体，也优先走聊天域

## 什么时候直接调 `/wunder`

- 承接 `user_id + question` 形式的直接执行请求
- 支持 SSE 流式输出和非流式 JSON 返回
- 允许调用方显式覆盖 `tool_names`、`model_name`、`config_overrides`
- 可接受 `agent_id`，但当前主要用于主会话绑定和工作区/容器路由

## 什么时候不要先调 `/wunder`

- 你要按 `agent_id` 自动补齐完整智能体快照
- 你要读取该会话当前实际冻结的 `system_prompt`
- 你要会话列表、取消、恢复、事件回放和聊天工作台能力
- 你要把某个独立人格智能体作为稳定产品接口发布

## 一条更稳的接入链路

推荐链路：

`GET /wunder/agents` -> `POST /wunder/chat/sessions` -> `POST /wunder/chat/sessions/{session_id}/messages`

1. `GET /wunder/agents`
2. 从返回的 `data.items[].id` 里选出目标 `agent_id`
3. `POST /wunder/chat/sessions` 创建该智能体下的新会话
4. `POST /wunder/chat/sessions/{session_id}/messages` 发送消息

这条链路的意义是：服务端会在聊天域里按会话绑定的 `agent_id` 自动补齐该智能体的配置快照。

## 需要传哪些身份字段

- `user_id`：表示调用者和隔离空间，可以是注册用户，也可以是在 API Key/管理员场景下使用的虚拟用户
- `agent_id`：表示目标智能体应用

外部调用方当前不需要显式传“智能体拥有者的 user_id”或额外的 owner 标识。

## 最小请求示例

```json
{
  "user_id": "demo_user",
  "question": "帮我整理今天的工作计划",
  "stream": true
}
```

常用补充字段：

- `session_id`：复用已有会话
- `agent_id`：绑定目标智能体作用域
- `model_name`：临时指定模型配置
- `tool_names`：显式启用工具
- `agent_prompt`：仅适合高级集成的低层提示词覆盖
- `attachments`：附件列表

## 最容易搞错的点

- `/wunder` 能传 `agent_id`，不等于它就是最稳的智能体发布协议。
- 直调 `/wunder` 时，如果想贴近某个智能体当前配置，很多默认项要你自己补。
- `user_id` 用来表示调用者和隔离空间，不要求一定是已注册用户。

## 相关接口

- `POST /wunder`
- `POST /wunder/system_prompt`
- `GET /wunder/tools`
- `GET /wunder/agents`
- `POST /wunder/chat/sessions`
- `POST /wunder/chat/sessions/{session_id}/messages`

## 相关文档

- [聊天会话](/docs/zh-CN/integration/chat-sessions/)
- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
- [A2A 接口](/docs/zh-CN/integration/a2a/)
- [API 索引](/docs/zh-CN/reference/api-index/)
