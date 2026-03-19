---
title: 部署与运行
summary: Wunder 的部署要先分清 desktop、本地开发和 server 三条路径。
read_when:
  - 你要部署 Wunder
  - 你要确认数据库、MCP、sandbox 和工作区怎么放
source_docs:
  - README.md
  - docs/API文档.md
  - docs/系统介绍.md
  - config/wunder-example.yaml
---

# 部署与运行

部署 Wunder 之前，先不要问“怎么启动”，先问“我要哪种运行形态”。

## 三条部署路径

### Desktop

适合个人直接使用。

特点：

- 本地优先
- 自带桌面壳
- 不要求先搭完整 server

### Server

适合团队、组织与统一治理。

特点：

- 多用户
- 多租户
- 管理端与用户端协同
- 可接 sandbox、MCP、A2A、渠道

### 本地开发

适合开发者联调。

特点：

- Rust 后端
- 前端 dev server
- 可单独调某一层

## Server 侧推荐组成

典型部署里，至少会涉及这些组件：

- `wunder-server`
- `wunder-sandbox`
- `extra-mcp`
- PostgreSQL

如果需要用户侧生产前端，通常还会有静态资源服务。

## 数据与存储要点

- 网页端 server 使用 PostgreSQL
- desktop 本地模式使用 SQLite
- 用户工作区需要持久化
- `data/` 是临时目录，不要把长期业务资料沉进去

## 关键配置文件

- `config/wunder.yaml`
- `config/wunder-example.yaml`
- `data/config/wunder.override.yaml`
- `extra_mcp/mcp_config.json`

## 常见环境变量

- `WUNDER_HOST`
- `WUNDER_PORT`
- `WUNDER_API_KEY`
- `WUNDER_POSTGRES_DSN`
- `WUNDER_SANDBOX_ENDPOINT`
- `WUNDER_MCP_HOST`
- `WUNDER_CONFIG_PATH`
- `WUNDER_CONFIG_OVERRIDE_PATH`

## 单端口与路径规划

当前系统已经支持把这些对外入口收口：

- `/wunder`
- `/a2a`
- `/.well-known/agent-card.json`
- `/docs/`

这意味着你可以把业务 API、协议入口和文档站统一挂在同一服务域名下。

## 启动后至少检查什么

建议按这个顺序检查：

1. `/wunder` 是否可返回
2. `/wunder/chat/ws` 是否可建连
3. `/a2a/agentCard` 是否可读
4. `/wunder/mcp` 是否可达
5. `/docs/` 是否能打开文档站

## 最容易忽略的部署问题

- 只启动了 server，但没准备好 Postgres
- 开启了 MCP 配置，但目标服务没通
- 工作区没有持久化，导致产物丢失
- 把长期业务数据放进了 `data/`
- 误把 desktop 本地模式当成 server 部署方式

## 相关文档

- [Server 部署](/docs/zh-CN/start/server/)
- [认证与安全](/docs/zh-CN/ops/auth-and-security/)
- [配置说明](/docs/zh-CN/reference/config/)
