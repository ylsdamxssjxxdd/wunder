---
title: 部署与运行
summary: 部署 Wunder 前先分清 desktop、本地开发和 server 三条路径，再谈启动命令。
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

部署 Wunder 之前，先不要急着问“怎么启动”，先问“我要哪种运行形态”。

## 本页重点

- 怎么先选对部署路径
- server 侧最少要准备哪些东西
- 启动后应该先查哪些入口

## 先定这四件事

- 你是个人本地使用，还是团队上线
- 数据库该用 PostgreSQL 还是 SQLite
- 工作区放在哪，是否持久化
- sandbox、MCP、静态前端要不要一起上线

## 按目标选部署路径

### Desktop

- 适合个人直接使用
- 本地优先
- 自带桌面壳
- 不要求先搭完整 server

### Server

- 适合团队、组织和统一治理
- 多用户、多租户
- 管理端与用户端协同
- 可接 sandbox、MCP、A2A 和外部渠道

### 本地开发

- 适合开发者联调
- Rust 后端和前端 dev server 分层调试
- 可以只启动某一层做局部验证

## 如果你部署 Server，最少要准备什么

典型部署里，至少会涉及这些组件：

- `wunder-server`
- PostgreSQL
- 持久化用户工作区

如果你需要更完整的能力，再按需接入：

- `wunder-sandbox`
- `extra-mcp`
- 用户侧或管理侧静态资源服务

## 对外路径怎么规划

- `/wunder`
- `/wunder/chat/*`
- `/a2a`
- `/.well-known/agent-card.json`
- `/docs/`

这些入口可以统一挂到同一服务域名下，别等上线前再临时拼接。

## 启动后先查这几项

1. `/wunder` 是否可返回
2. `/wunder/chat/ws` 是否可建连
3. `/a2a/agentCard` 是否可读
4. `/wunder/mcp` 是否可达
5. `/docs/` 是否能正常打开

## 关键配置文件

- `config/wunder.yaml`
- `config/wunder-example.yaml`
- `data/config/wunder.override.yaml`
- `config/mcp_config.json`

## 常见环境变量

- `WUNDER_HOST`
- `WUNDER_PORT`
- `WUNDER_API_KEY`
- `WUNDER_POSTGRES_DSN`
- `WUNDER_SANDBOX_ENDPOINT`
- `WUNDER_MCP_HOST`
- `WUNDER_CONFIG_PATH`
- `WUNDER_CONFIG_OVERRIDE_PATH`

## Docker 下的浏览器运行时

- 当前 Compose 默认会在镜像构建阶段安装 Playwright Chromium
- 当前 Compose 默认会开启 `WUNDER_BROWSER_ENABLED=true`、`WUNDER_BROWSER_TOOL_ENABLED=true` 和 `WUNDER_BROWSER_DOCKER_ENABLED=true`
- `shm_size: 2gb` 是给 Chromium 的 `/dev/shm` 预留空间，避免容器内因为共享内存过小出现崩溃、卡死、空白页或截图失败

## 最容易忽略的部署问题

- 只启动了 server，但没准备好 Postgres
- 开启了 MCP 配置，但目标服务没通
- 工作区没有持久化，导致产物丢失
- 把长期业务数据放进了 `data/`
- 误把 desktop 本地模式当成 server 部署方式

## 延伸阅读

- [Server 部署](/docs/zh-CN/start/server/)
- [认证与安全](/docs/zh-CN/ops/auth-and-security/)
- [配置说明](/docs/zh-CN/reference/config/)
