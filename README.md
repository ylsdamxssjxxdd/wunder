# wunder 心舰

一个智能体调度系统。你把目标交给它，它负责拆解任务、调用工具、交付结果。

前端是温暖的蜂巢，后端是庞大的飞船。

## 当前状态

项目处于原型阶段，接口和结构可能随时调整。

三种运行形态的进度不一样：

- **server**：项目核心，功能最全，也是日常开发的主战场
- **desktop**：开发中，功能还不全；基于 Slint 的本地桌面应用，个人用户的主推形态
- **cli**：开发中，功能还不全；复用 server 内核的命令行形态

目前想完整体验，建议从 server 入手。desktop 和 cli 能跑起来，但别期待它们已经覆盖 server 的全部能力。

## 基本概念

```text
心舰（wunder 平台）
├─ 用户
│  └─ 蜂群
│     └─ 智能体
│        └─ 线程
└─ ...
```

- 用户：隔离与资源归属的顶层主体
- 蜂群：围绕一个目标组织起来的一组智能体
- 智能体：具体干活的角色，负责规划、调模型、调工具
- 线程：智能体内部连续的执行上下文

目标落到用户，在蜂群里分工，由智能体在各自线程里执行。

## 三种形态

**wunder-server** 是核心。多用户、多租户、智能体应用构建与发布、网关统一接入，内置工具链、知识库和长期记忆。建议部署在 Linux 上获得最佳性能。

**wunder-desktop**（开发中）是本地桌面形态，默认前端是 Rust + Slint，启动后在同一进程内直接调用后端，不依赖本机 HTTP。兼容目标是 Windows 7 32 位。

**wunder-cli**（开发中）是命令行形态，复用 runtime 核心，不另造一套执行链路。

三种形态共享同一个核心：线程、工具、存储、实时事件和权限语义是同一套代码，只有接入层不同。

## 技术栈

后端统一是 Rust workspace（tokio 异步运行时），按形态分：

| | server | desktop | cli |
| :--- | :--- | :--- | :--- |
| 后端 | Rust + axum 0.8 | Rust，复用 runtime 内核，同进程原生调用 | Rust，复用 runtime 内核 |
| 界面 | Vue 3 + TypeScript（Vite 构建） | Slint 1.18 原生界面，软件渲染 | 终端 TUI（ratatui + crossterm） |
| 管理端 | 原生 HTML + JS（web/） | — | — |
| 数据库 | PostgreSQL | SQLite（rusqlite） | SQLite（rusqlite） |
| 接入方式 | HTTP / WebSocket | 进程内直调，不经本机网络 | 本地进程 |
| 兼容目标 | Linux / Docker | Windows 7 x86 起，另出 Linux AppImage | Windows 7 及以上（GNU 工具链构建） |

CLI 参数解析用 clap，TLS 用 rustls（ring provider）。桌面端使用原生 Slint 前端与共享的 Rust 运行时。

## 能力概况

- 多智能体并行协作与任务交接
- MCP 工具接入，Skills 固化流程
- 定时任务、网关与多渠道接入
- 长会话上下文压缩与长期记忆
- 知识库

## 文档

- 使用说明书（用户/管理员/开发者）：`docs/使用说明书/zh-CN/index.md`
- 系统简介：`docs/wunder系统简介.md`
- 总体设计：`docs/总体设计.md`
- API 文档：`docs/API文档.md`

## 已吞噬项目

wunder 在开发过程中吸收了不少开源项目的代码和思路：

| 吞噬 | 项目 | 地址 |
| :--- | :--- | :--- |
| 项目原型 | EVA | https://github.com/ylsdamxssjxxdd/eva |
| 智能体基础 | OpenAI Codex | https://github.com/openai/codex |
| 前端基础 | HuLa | https://github.com/HuLaSpark/HuLa |
| 协议基础 | Claude Code | https://github.com/anthropics/claude-code |
| 用户基础 | OpenClaw | https://github.com/openclaw/openclaw |
