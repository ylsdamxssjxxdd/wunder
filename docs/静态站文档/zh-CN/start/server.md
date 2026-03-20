---
title: Server 部署
summary: 需要多用户治理、统一接入层和管理员后台时，再看 `wunder-server` 这条线。
read_when:
  - 你要部署 server 形态
  - 你需要多用户、组织治理和对外接入能力
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
  - docs/API文档.md
---

# Server 部署

如果你已经确定要做团队部署、管理员后台或统一对外接口，这页再看。

`wunder-server` 是 Wunder 的核心运行形态，负责多用户、多单位、多租户治理，以及 `/wunder`、`/wunder/chat/*`、`/a2a` 等公开入口。

## 这页解决什么

- 什么时候必须上 server
- server 负责哪些能力
- 部署前要先定哪些关键项

## 先记这几条

- `server` 是团队和组织场景的主入口，不是个人本地模式的替代叫法。
- 网页端 server 形态使用 PostgreSQL，不是 SQLite。
- `user_id` 不要求一定是已注册用户，外部调用也可以传虚拟用户标识。
- 实时链路优先 WebSocket，SSE 作为兜底。

## 什么时候必须上 Server

- 你要多用户并发访问
- 你要组织、单位、租户和管理员治理
- 你要统一暴露 `/wunder`、聊天接口和 `A2A`
- 你要把 Wunder 当平台能力接给别的系统

## Server 负责什么

- `/wunder` 底层执行入口
- `/wunder/chat/*` 完整智能体会话入口
- `/a2a` 系统级 A2A 接口
- WebSocket 和 SSE 流式能力
- 用户与管理员接口
- 智能体调度与工具编排
- 用户、单位、权限与治理能力

## 部署前先定这些事

- 数据库放 PostgreSQL，连通性和备份方案先定好
- 用户工作区要持久化，不要落到临时目录
- 需不需要同时部署 sandbox、MCP、A2A 和静态文档站
- 对外域名下准备暴露哪些路径

## 对外访问先记住这些

- 业务方可以把 `/wunder` 当成底层执行入口
- 如果要稳定调用某个独立人格智能体，优先走 `/wunder/chat/*`
- `user_id` 不要求一定是注册用户
- A2A 当前暴露的是系统级路由卡，不是每个智能体一张独立卡

## 最容易搞错的点

- 把 `desktop` 当成 server 的部署方式，这是两条线。
- 只暴露了 `/wunder`，却没规划聊天域和 WebSocket。
- 误以为 `user_id` 必须先在用户管理里注册。
- 把长期业务数据放进 `data/`，后面很容易被清理或覆盖。

## 相关文档

- [部署与运行](/docs/zh-CN/ops/deployment/)
- [认证与安全](/docs/zh-CN/ops/auth-and-security/)
- [系统架构](/docs/zh-CN/concepts/architecture/)
- [管理端界面](/docs/zh-CN/surfaces/web-admin/)
- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
