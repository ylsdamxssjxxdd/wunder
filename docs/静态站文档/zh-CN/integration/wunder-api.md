---
title: "wunder API"
summary: "`/wunder` 是 Wunder 的底层执行入口；如果你要稳定调用独立人格智能体，优先走 `/wunder/chat/*`。"
read_when:
  - "你要从业务系统接入 Wunder"
  - "你需要判断该走 `/wunder` 还是 `/wunder/chat/*`"
source_docs:
  - "docs/API文档.md"
  - "docs/系统介绍.md"
  - "docs/设计方案.md"
---

# wunder API

`POST /wunder` 现在更适合作为底层执行内核和调试入口，不应再把它理解为“调用任意独立人格智能体的唯一公开接口”。

> 推荐链路：
> `GET /wunder/agents` -> `POST /wunder/chat/sessions` -> `POST /wunder/chat/sessions/{session_id}/messages`

## `/wunder` 负责什么

- 承接 `user_id + question` 形式的直接执行请求
- 支持 SSE 流式输出和非流式 JSON 返回
- 允许调用方显式覆盖 `tool_names`、`model_name`、`config_overrides`
- 可接受 `agent_id`，但当前主要用于主会话绑定和工作区/容器路由

## `/wunder` 当前不负责什么

- 不会像 `/wunder/chat/*` 一样按 `agent_id` 自动补齐完整智能体快照
- 不会自动解析目标智能体当前的 `system_prompt`、模型、工具默认集和审批模式
- 不应被当成“对外发布单个智能体人格”的稳定调用协议

如果你必须直调 `/wunder`，又想尽量贴近某个智能体当前行为，需要自己传入 `agent_prompt` 等覆盖字段；更稳妥的方式仍然是改走聊天域。

## 推荐的开发者接入流程

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

## 相关接口

- `POST /wunder`
- `POST /wunder/system_prompt`
- `GET /wunder/tools`
- `GET /wunder/agents`
- `POST /wunder/chat/sessions`
- `POST /wunder/chat/sessions/{session_id}/messages`

## 什么时候优先不用 `/wunder`

如果你的目标是下面这些场景，优先使用 `/wunder/chat/*`：

- 完整聊天 UI
- 需要稳定复用某个独立人格智能体
- 需要会话列表、取消、恢复、事件回放
- 需要读取“该会话当前实际使用的系统提示词”

`/wunder` 更像底层执行入口；`/wunder/chat/*` 更像完整聊天域和智能体会话控制面。

## 相关文档

- [聊天会话](/docs/zh-CN/integration/chat-sessions/)
- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
- [A2A 接口](/docs/zh-CN/integration/a2a/)
- [API 索引](/docs/zh-CN/reference/api-index/)
