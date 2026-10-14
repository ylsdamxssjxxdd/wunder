# wunder 心舰

wunder（心舰）是一个**智能体调度平台**：把目标交给智能体，它拆解任务、调用工具、交付结果。

四个形态各有定位：蛹（desktop）是**本地 AI 工作台**，个人用户的默认入口与主推应用；蜂巢（web）是**云端 AI 工作台**，随舰体提供；舵机（cli）是**手工本地操作入口**，终端驱动、脚本化；舰体（server）是**用户与智能体管理平台**，支持多租户、用户与单位管理、智能体应用构建与发布、网关统一接入与调度，并内置工具链、知识库与长期记忆。舰桥（`web/`）是舰体自带的治理界面，面向管理员。

## 形态矩阵

| 程序图标 | 形态名称 | 技术栈 | 定位 | 运行方式 | 系统支持 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 🚀 | 舰体 server | Rust + axum 0.8；PostgreSQL | 用户与智能体管理平台（多租户、用户与单位、智能体应用构建发布、网关接入与调度） | 服务端常驻进程，Docker 部署，HTTP / WebSocket 接入 | Linux / Docker |
| 🐉 | 舰桥 web | 原生 HTML + CSS + JavaScript（`web/`，无构建） | 舰体的治理后台（系统、智能体、调试、文档四组面板） | 浏览器访问，由舰体静态挂载于 `/` | 跨平台浏览器 |
| 🐝 | 蜂巢 web | Vue 3 + TypeScript（Vite 构建，`frontend/`） | 云端 AI 工作台（舰体的用户端） | 浏览器访问；构建产物 `frontend/dist` 由 nginx 容器只读挂载对外服务（默认端口 18001），**不由舰体托管** | 跨平台浏览器 |
| 🍯 | 蛹 desktop | Rust + Slint 1.18（默认前端 `frontend-slint/`，原生界面、软件渲染）；SQLite | 本地 AI 工作台（主推形态，开箱即用） | 本地桌面进程，同进程原生直调引擎（不经本机网络）；Electron / Tauri 壳已停止维护 | Windows 7 x86 起；Linux（ubuntu18.04 基线）x64 / arm64 AppImage |
| ⚙️ | 舵机 cli | Rust 终端 TUI（ratatui + crossterm）；SQLite | 手工本地操作入口（终端、脚本化、自动化） | 本地终端进程，同进程直调引擎，支持 JSONL 输出 | Windows 7 起（GNU 工具链）；Linux amd64 / arm64（ubuntu18.04 sysroot） |

各形态共享同一引擎（wunder-runtime）与核心（wunder-core）：线程、工具、存储抽象、实时事件和权限语义是同一份代码，差异只在接入层。蛹与舵机不依赖舰体，可独立运行与分发。

一个智能体可同时拥有多个独立任务线程，调度、锁与实时状态均按任务线程隔离；工作区可为本地目录、云端 `/workspaces/<user>/workspace` 与远程设备工作区并存。

## 性能矩阵

用同一套引擎的不同形态，其性能特征对比如下。数值待实测后填写；采样口径与回退门槛见 [性能标准](docs/性能标准.md)，评分口径见 [性能评分体系](docs/性能评分体系.md)（三形态独立计分、不可跨形态相加），历史数据见 [性能基线](docs/性能基线/)。

| 程序图标 | 形态名称 | 启动速度 | 内存占用 | CPU 占用 | 包体积 | 并发智能体线程 | 聊天页面性能 |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| 🚀 | 舰体 server | — | — | — | — | — | — |
| 🐉 | 舰桥 web | — | — | — | — | — | — |
| 🐝 | 蜂巢 web | — | — | — | — | — | — |
| 🍯 | 蛹 desktop | — | — | — | — | — | — |
| ⚙️ | 舵机 cli | — | — | — | — | — | — |

> 采集脚本见 [scripts/form-bench](scripts/form-bench/README.md)：一次运行即可产出上表全部指标。

## 工具矩阵

模型看到的工具规范名大多为中文，另配英文别名，由 `resolve_tool_name` 归一（`crates/wunder-runtime/src/services/tools/catalog.rs`）。MCP 工具（`server@tool`）、技能名、知识库名与用户工具别名按配置动态注册，不在下表之列。

| 工具（规范名） | 英文别名 | 简介 | 分类 |
| :--- | :--- | :--- | :--- |
| 计划面板 | `update_plan` | 向前端推送结构化的任务计划面板 | 收尾与前端协同 |
| 问询面板 | `question_panel` | 弹出结构化选项，向用户提问并等待选择 | 收尾与前端协同 |
| 会话让出 | `sessions_yield` | 主动让出当前回合控制权，稍后再被唤起 | 收尾与前端协同 |
| 列出文件 | `list_files` | 列出目录内容 | 工作区与代码 |
| glob | — | 按通配模式查找文件路径 | 工作区与代码 |
| 搜索内容 | `search_content` | 在文件中按正则搜索匹配内容 | 工作区与代码 |
| 读取文件 | `read_file` | 读取文件内容，支持分段读取 | 工作区与代码 |
| 写入文件 | `write_file` | 写入或整体覆盖文件 | 工作区与代码 |
| 文本编辑 | `edit` / `edit_file` | 精确替换 / 补丁 / 子命令三形态（`apply_patch` 已并入） | 工作区与代码 |
| 执行命令 | `execute_command` | 执行 Shell / 系统命令 | 工作区与代码 |
| 命令会话 | `command_session` | 管理后台长驻命令会话，支持前后台切换 | 工作区与代码 |
| 技能调用 | `skill_call` | 读取技能正文并执行技能脚本 | 工作区与代码 |
| 网页抓取 | `web_fetch` | 抓取并解析网页正文（需启用配置） | Web |
| 浏览器 | `browser` | 聚合展示名；底层为 17 个 `browser_*` 原生工具（点击、输入、等待、截图等） | Web |
| 读图工具 | `read_image` | 读取图片内容供模型理解（需 vision 能力） | 多模态 |
| 图像生成 | `generate_image` | 按提示词生成图片 | 多模态 |
| 视频生成 | `generate_video` | 按提示词生成视频 | 多模态 |
| 语音生成 | `generate_speech` | 文本转语音 | 多模态 |
| 声转文 | `transcribe_speech` | 语音转文字 | 多模态 |
| 桌面监视器 | `desktop_monitor` | 读取桌面窗口与屏幕状态 | 桌面 |
| 桌面控制器 | `desktop_controller` | 操作本机桌面（鼠标、键盘、窗口） | 桌面 |
| 子智能体控制 | `subagent_control` | 在任务中临时派生子智能体，结果回传后回收 | 线程与子智能体 |
| 会话线程控制 | `thread_control` | 管理任务线程 / 分支线程（桌面端不开放） | 线程与子智能体 |
| 自我状态 | `self_status` | 查询智能体自身的运行状态 | 系统与记忆 |
| 记忆管理 | `memory_manager` | 读写长期记忆 | 系统与记忆 |
| 定时任务 | `schedule_task` | 创建与管理定时 / 周期任务 | 系统与记忆 |
| get_goal / create_goal / update_goal | — | 目标态三件套，由运行时按会话自动注入，不对桌面端开放选择 | 目标态 |

> `网页搜索` 与 `ptc` 属代码保留、当前**临时下架**（对模型与用户不可见，见 `default_tool_profile.rs`），不是已移除。已彻底移除：渠道工具、用户世界工具、`a2a_*` 工具、`final_response`、`a2ui`、LSP 查询类工具。

## 文档入口

- 系统总览：[docs/wunder系统简介.md](docs/wunder系统简介.md)；总体设计：[docs/总体设计.md](docs/总体设计.md)
- 使用说明书（用户 / 管理员 / 开发者）：[docs/使用说明书/zh-CN/index.md](docs/使用说明书/zh-CN/index.md)
- API 文档：[docs/API文档.md](docs/API文档.md)
- 技术说明书：[docs/技术说明书/README.md](docs/技术说明书/README.md)；设计文档：[docs/设计文档/README.md](docs/设计文档/README.md)
- 功能迭代记录：[docs/功能迭代.md](docs/功能迭代.md)；经验教训：[docs/经验教训.md](docs/经验教训.md)

对外集成面：REST `/wunder/*` 与 WebSocket 六个实时通道、MCP 端点 `/wunder/mcp`、外部工作流 SSE `/wunder/external/workflow*`、渠道 webhook。

## 吞噬矩阵

| 吞噬 | 项目 | 地址 |
| :--- | :--- | :--- |
| 项目原型 | EVA | https://github.com/ylsdamxssjxxdd/eva |
| 智能体基础 | OpenAI Codex | https://github.com/openai/codex |
| 前端基础 | HuLa | https://github.com/HuLaSpark/HuLa |
| 协议基础 | Claude Code | https://github.com/anthropics/claude-code |
| 用户基础 | OpenClaw | https://github.com/openclaw/openclaw |
| 工具基础 | deepseek-harness（dsh） | https://github.com/deepseek-ai/deepseek-harness |
