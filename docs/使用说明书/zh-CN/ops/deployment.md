---
title: 部署与运行
summary: 部署前先想清楚用哪种形态、准备什么环境。Server 部署至少需要数据库、工作区和后端服务。
---

# 部署与运行

## 先选形态

部署前先回答一个问题：**用户打算怎么用？**

| 形态 | 适合谁 | 需要什么 |
|------|--------|----------|
| **Desktop** | 个人使用 | 下载安装包，开箱即用 |
| **Server** | 团队/组织 | 服务器 + 数据库 + 配置 |
| **本地开发** | 开发调试 | 本地编译环境 |

## Desktop 部署

最简单的方式：
1. 下载安装包
2. 安装并启动
3. 开始使用

不需要额外配置。数据存在本地，使用 SQLite。

详细步骤见 [Desktop 入门](/docs/zh-CN/start/desktop/)。

## Server 部署

### 最低要求

| 组件 | 说明 | 必须？ |
|------|------|--------|
| 后端服务 | wunder-server | 是 |
| PostgreSQL | 主数据库 | 是 |
| 工作区存储 | 用户文件空间 | 是 |
| 前端静态资源 | 用户端/管理端页面 | 推荐 |
| MCP 服务 | 外部工具能力 | 按需 |
| 沙盒服务 | 隔离执行环境 | 按需 |
| 向量数据库 | 知识库检索 | 按需 |

### 推荐部署方式

**Docker Compose**（推荐）：
- 一键启动所有组件
- 配置统一管理
- 升级方便

### 网络规划

部署前先规划好对外的网络入口：

| 入口 | 用途 |
|------|------|
| 主服务地址 | 后端接口 |
| WebSocket 通道 | 实时通信 |
| 管理端地址 | 管理后台 |
| 文档站地址 | 使用说明书 |

建议统一在同一域名下，通过路径区分。

### 启动后检查

启动完成后，按这个清单检查：

1. 后端服务是否正常运行
2. 数据库连接是否正常
3. WebSocket 是否可建立连接
4. 管理端是否可打开
5. 用户端是否可访问
6. 文档站是否可访问

### 关键环境变量

| 变量 | 作用 |
|------|------|
| `WUNDER_HOST` | 服务监听地址 |
| `WUNDER_PORT` | 服务端口 |
| `WUNDER_API_KEY` | API 密钥 |
| `WUNDER_POSTGRES_DSN` | PostgreSQL 连接串 |
| `WUNDER_CONFIG_PATH` | 配置文件路径 |
| `WUNDER_SANDBOX_ENDPOINT` | 沙盒服务地址 |
| `WUNDER_SERVER_FEATURES` | Docker 下 Rust 服务编译特性，默认 `mcp,host-metrics,web-fetch` |
| `WUNDER_SANDBOX_DOCKER_READ_ONLY` | Docker Compose 下 sandbox 容器级只读根文件系统开关，默认 `false` |

### Docker 下的系统状态

管理员侧系统状态中的 CPU、内存、进程、负载和磁盘指标依赖 `host-metrics` 编译特性，`网页抓取` 依赖 `web-fetch` 编译特性。当前 Compose 默认使用 `WUNDER_SERVER_FEATURES=mcp,host-metrics,web-fetch`；如果在 `.env` 中手动覆盖该变量，需要保留 `host-metrics` 和 `web-fetch`，否则系统资源指标会以 0 值降级显示，或用户侧智能体工具列表不会显示 `网页抓取`。

管理员侧 Firecrawl 设置会同步给 `网页搜索`：当抓取 provider 为 `firecrawl`，或为 `auto` 且已经配置 Firecrawl API Key/自定义地址时，用户侧智能体工具列表会显示 `网页搜索`。

### Docker 下的浏览器

如果需要在 Docker 中使用浏览器工具：
- 确保镜像构建时安装了 Chromium
- 分配足够的共享内存（`shm_size: 2gb`）
- 启用浏览器相关环境变量

### Docker 下的沙盒写权限

当前 Compose 默认让 `wunder-sandbox` 保持可写根文件系统，方便文件工具访问容器内任意路径。`WUNDER_SANDBOX_READONLY_ROOTFS` 是 Wunder 请求层的沙盒只读开关；Docker 自身的容器级 `read_only` 由 `WUNDER_SANDBOX_DOCKER_READ_ONLY` 控制。若文件工具写 `/test_file.txt` 这类根路径时报 `Read-only file system (os error 30)`，优先检查正在运行的容器是否仍使用旧的 `read_only: true` 配置，并重建 `wunder-sandbox` 容器。

## 升级策略

- 滚动更新：逐个替换实例，不中断服务
- 数据库迁移：系统自动处理，不需要手动操作
- 配置兼容：新版本会自动适配旧配置格式

## 常见部署问题

| 问题 | 原因 | 解决 |
|------|------|------|
| 启动失败 | PostgreSQL 未准备好 | 确保数据库先启动 |
| WebSocket 连不上 | 端口或代理配置问题 | 检查端口和反向代理配置 |
| 工作区丢失 | 没有持久化存储 | 使用数据卷挂载 |
| 临时文件堆积 | 没有清理策略 | 配置定期清理任务 |
| 管理端系统状态资源指标为 0 | `WUNDER_SERVER_FEATURES` 覆盖后缺少 `host-metrics` | 恢复为 `mcp,host-metrics,web-fetch` 并重建 server/sandbox |
| 用户侧看不到网页抓取 | `WUNDER_SERVER_FEATURES` 覆盖后缺少 `web-fetch` | 恢复为 `mcp,host-metrics,web-fetch` 并重建 server/sandbox |
| 用户侧看不到网页搜索 | Firecrawl 搜索 provider 未启用 | 在管理员侧将网页抓取 provider 设为 `firecrawl`，或使用 `auto` 并配置 Firecrawl 地址/API Key |
| 浏览器工具不可用 | Docker 未安装 Chromium | 检查镜像构建配置 |
| 文件工具根路径写入失败 | sandbox 容器仍是只读根文件系统 | 确认 `WUNDER_SANDBOX_DOCKER_READ_ONLY=false` 并重建 sandbox 容器 |

## 延伸阅读

- [Server 入门](/docs/zh-CN/start/server/)
- [数据与存储](/docs/zh-CN/ops/data-and-storage/)
- [系统设置参考](/docs/zh-CN/reference/config/)
