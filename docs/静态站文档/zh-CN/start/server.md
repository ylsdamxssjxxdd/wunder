---
title: Server 部署
summary: `wunder-server` 是系统核心，负责多租户治理、智能体调度，以及 `/wunder`、`/wunder/chat/*`、`/a2a` 等对外接口。
read_when:
  - 你要部署 server 形态
  - 你需要多用户、组织治理和对外接入能力
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
  - docs/API文档.md
---

# Server 部署

`wunder-server` 是 Wunder 的核心运行形态。

它面向多用户、多单位、多租户、管理员治理，以及对外开放接口。

## Server 提供什么

- `/wunder` 底层执行入口
- `/wunder/chat/*` 完整智能体会话入口
- `/a2a` 系统级 A2A 接口
- WebSocket 和 SSE 流式能力
- 用户与管理员接口
- 智能体调度与工具编排
- 用户、单位、权限与治理能力

## 适合什么场景

- 团队或组织内部部署
- 需要统一接入层和管理员后台
- 需要多用户并发与长期持久化工作区
- 需要把 Wunder 作为平台能力开放给其他系统

## 对外访问的关键点

- 业务方可以把 `/wunder` 当成底层执行入口
- 如果要稳定调用某个独立人格智能体，优先走 `/wunder/chat/*`
- `user_id` 不要求一定是注册用户
- A2A 当前暴露的是系统级路由卡，不是每个智能体一张独立卡

## 数据与存储约定

- 网页端 server 形态使用 PostgreSQL
- desktop 本地形态使用 SQLite
- 用户工作区应持久化保存
- `data/` 目录视为运行时临时目录，不用于长期业务资料沉淀

## 部署前建议先看

- [部署与运行](/docs/zh-CN/ops/deployment/)
- [认证与安全](/docs/zh-CN/ops/auth-and-security/)
- [wunder API](/docs/zh-CN/integration/wunder-api/)

## 相关文档

- [系统架构](/docs/zh-CN/concepts/architecture/)
- [管理端界面](/docs/zh-CN/surfaces/web-admin/)
- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
