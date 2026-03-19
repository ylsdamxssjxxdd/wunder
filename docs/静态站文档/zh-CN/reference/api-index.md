---
title: API 索引
summary: 这是 Wunder 当前最常用接口族的索引页，用来快速定位入口而不是替代完整 API 文档。
read_when:
  - 你在找某个接口大概属于哪一类
  - 你不想直接翻完整 `docs/API文档.md`
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
---

# API 索引

这不是完整 API 手册，而是 Wunder 当前主要接口族的索引页。

## 核心执行入口

- `POST /wunder`
- `POST /wunder/system_prompt`
- `GET /wunder/tools`

适合先看：

- [wunder API](/docs/zh-CN/integration/wunder-api/)

## 聊天会话域

- `GET/POST /wunder/chat/sessions`
- `POST /wunder/chat/sessions/{session_id}/messages`
- `GET /wunder/chat/sessions/{session_id}/resume`
- `POST /wunder/chat/sessions/{session_id}/cancel`
- `GET /wunder/chat/ws`

适合先看：

- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)

## A2A

- `POST /a2a`
- `GET /.well-known/agent-card.json`
- `GET /a2a/agentCard`
- `GET /a2a/extendedAgentCard`

适合先看：

- [A2A 接口](/docs/zh-CN/integration/a2a/)

## MCP

- `POST /wunder/mcp`
- `GET/POST /wunder/admin/mcp`
- `POST /wunder/admin/mcp/tools`
- `POST /wunder/admin/mcp/tools/call`

适合先看：

- [MCP 入口](/docs/zh-CN/integration/mcp-endpoint/)

## 用户世界

- `GET /wunder/user_world/contacts`
- `GET /wunder/user_world/groups`
- `GET /wunder/user_world/conversations`
- `GET /wunder/user_world/ws`

## 管理接口

大部分管理接口都在：

- `/wunder/admin/*`

它们覆盖：

- 模型
- 工具
- 用户与组织
- 预设智能体
- benchmark
- 渠道治理

## 什么时候回到完整 API 文档

当你需要这些信息时，就该回完整文档了：

- 字段级请求体
- 响应结构
- 错误码
- 鉴权细节
- 历史兼容字段

## 相关文档

- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [配置说明](/docs/zh-CN/reference/config/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
