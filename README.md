# wunder 心舰

wunder（心舰）是一个 AI 工作台：你把目标交给智能体，它把目标拆成任务，调用工具一步步执行，最后交付结果——写进工作区的文件、执行过的命令、改好的代码、发出去的消息。过程有事件流可以看，高风险动作有审批把着，断了线能恢复现场。

它和常见的问答式 AI 不是一类东西。问答式 AI 的产出是一段文字，wunder 的产出是做完的事。

## 下载蜂窝（desktop），打开即用

**蜂窝（desktop）** 是 wunder 的桌面版，也是主推的应用。个人用户下载安装即可使用，不需要先部署任何服务：

1. **下载安装**：去 [Releases](../../releases) 下载匹配系统的安装包，安装后启动
2. **配置模型**：打开「系统设置」→「模型配置」，填入 API Key 和接口地址，点「测试连接」确认能用
3. **发起对话**：输入你的目标，例如 `帮我整理当前目录的文件并生成清单`，回车发送

智能体开始规划、调用工具、交付结果。完整上手说明见[蜂窝入门](docs/使用说明书/zh-CN/start/desktop.md)。

蜂窝在本地运行，共享同一套引擎（wunder-runtime）与核心（wunder-core），支持本地文件、窗口与浏览器操作，SQLite 存储，兼容 Windows 7 x86 起，另提供 Linux AppImage。

## 能做什么

- **文件与代码**：读取文件、编辑代码、执行命令、重构项目
- **办公自动化**：整理文档、生成报告、处理表格、做会议纪要
- **任务拆分**：需要并行推进时，由智能体临时派生子智能体分头执行，结果回传后汇总
- **持续任务**：定时巡检、周期提醒、跨渠道消息处理
- **工具生态**：内置工具链 + MCP 外部工具 + Skills 固化流程 + 知识库
- **长会话**：上下文压缩与长期记忆，长会话可以一直工作下去

## 基本概念

```text
心舰（wunder 平台）
├─ 用户
│  └─ 智能体
│     └─ 线程
└─ ...
```

- 用户：隔离与资源归属的顶层主体，每个用户只有唯一智能体实例
- 智能体：具体干活的角色，负责规划、调模型、调工具
- 工作区：智能体干活的工作目录，工具以它为根。蜂窝里是本地目录，可直接读写本机文件；舰体部署时是服务端的用户目录
- 线程：智能体内部连续的执行上下文
- 子智能体：智能体在任务中临时派生的工作单元，任务结束即回收

目标落到用户，由智能体在各线程里执行，需要拆分时派生临时子智能体。

## 进阶：服务版与命令版

蜂窝之外，wunder 还提供两种可选形态，按需取用：

**舰体（server）——服务版**。需要多人共用、组织治理时才部署。支持多租户、用户与单位管理、智能体应用构建与发布、网关统一接入与调度，内置工具链、知识库与长期记忆，PostgreSQL 存储，Docker 部署。包含管理前端（舰桥）和用户前端（蜂巢）。见[舰体部署](docs/使用说明书/zh-CN/start/server.md)。

**舵机（cli）——命令版**。终端与自动化场景使用，与蜂窝共享同一引擎。TUI 交互，支持 JSONL 输出，便于脚本和管道集成。见[舵机使用](docs/使用说明书/zh-CN/start/cli.md)。

三种形态共享同一套引擎（wunder-runtime）：线程、工具、存储抽象、实时事件和权限语义是同一份代码，差异只在接入层。

## 技术栈

| | 蜂窝 desktop | 舰体 server | 舵机 cli |
| :--- | :--- | :--- | :--- |
| 后端 | Rust，复用引擎（wunder-runtime），同进程原生调用 | Rust + axum 0.8 | Rust，复用引擎（wunder-runtime） |
| 界面 | Slint 1.18 原生界面，软件渲染 | Vue 3 + TypeScript（Vite 构建）；舰桥为原生 HTML + JS（web/） | 终端 TUI（ratatui + crossterm） |
| 数据库 | SQLite（rusqlite） | PostgreSQL | SQLite（rusqlite） |
| 接入方式 | 进程内直调，不经本机网络 | HTTP / WebSocket | 本地进程 |
| 兼容目标 | Windows 7 x86 起，另出 Linux AppImage | Linux / Docker | Windows 7 及以上（GNU 工具链构建） |

后端统一是 Rust workspace（tokio 异步运行时）：`wunder-core`（配置、鉴权、存储契约、执行策略）→ `wunder-runtime`（orchestrator、线程运行时、工具、渠道、网关），三种形态都构建在引擎之上，蜂窝与舵机不依赖舰体。舵机参数解析用 clap，TLS 用 rustls（ring provider）。

## 文档

- 使用说明书（用户/管理员/开发者）：`docs/使用说明书/zh-CN/index.md`
- 系统简介：`docs/wunder系统简介.md`
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
