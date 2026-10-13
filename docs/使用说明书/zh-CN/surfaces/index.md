---
title: 界面概览
summary: 四个形态对应四个界面：蜂窝（本地 AI 工作台）、蜂巢（云端 AI 工作台）、舵机（终端 TUI）、舰桥（治理后台）。按形态查界面能力。
read_when:
  - 想知道某个能力在哪个界面上有、在哪个界面上没有
  - 想理解蜂窝 / 蜂巢 / 舵机 / 舰桥的分工与差异
source_docs:
  - frontend-slint/ui/theme.slint
  - frontend-slint/ui/settings_page.slint
  - frontend/src/router/index.ts
  - web/index.html
  - crates/wunder-cli/tui/app.rs
updated_at: 2026-10-13
---

# 界面概览

wunder 的界面按形态分四类：**蜂窝**（本地 AI 工作台）、**蜂巢**（云端 AI 工作台）、**舵机**（终端 TUI）、**舰桥**（管理员治理后台）。四者共享同一引擎，但接入面、可用能力与呈现方式各不相同。

## 四个界面各自是什么

| 界面 | 面向 | 定位 | 载体 |
| --- | --- | --- | --- |
| **蜂窝** | 个人用户 | 本地 AI 工作台，主推应用 | 桌面可执行 `wunder-frontend-slint`（Rust + Slint），无端口，同进程原生调用引擎 |
| **蜂巢** | 团队成员 | 云端 AI 工作台 | 浏览器访问，`frontend/`（Vue3 + TS）单页应用；构建产物 `frontend/dist` 由独立 **nginx 容器**下发（默认 18001），不由舰体进程托管 |
| **舵机** | 终端与自动化用户 | 手工本地操作入口 | `crates/wunder-cli/`；TTY 进 ratatui TUI，非 TTY 降级为一次性 `exec` |
| **舰桥** | 管理员 | 治理与调试后台 | `web/`（原生 HTML + JS 模块），无构建步骤，由舰体 `mount_static(app, "web", "/")` 挂在同一端口（默认 18000） |

## 每个界面的「主区域」是什么样的

**蜂窝**主窗**只有两页**（`ui/theme.slint:144` `enum DesktopPage { messages, settings }`）：对话页与设置页。其余能力以**面板与对话框**呈现：侧栏、输入区、时间线、终端视图、工具管理对话框、形象对话框、工作区对话框、工作目录区、线程轨迹、子智能体详情、渠道页、定时页、设置分栏、活动卡、专家数据、补丁差异卡、截图遮罩、托盘与系统通知。

设置页导航有 **12 个分类**（`ui/settings_page.slint:672-750`）：通用、账户、模型、工具、智能体、形象、定时、记忆、渠道、提示词包、归档、帮助。**没有「设备互通」类目**。

**蜂巢**是 SPA，路由集合（`frontend/src/router/index.ts:234-298`）：`/login`、`/register`、`/home`、`/portal`、`/app/{home,external/:linkId,chat,embed/chat,settings,trajectory}`、`/demo/*`、`/admin/login`、`/admin/{users,agents,system}`；另有 6 条 `/__e2e/*` 压测壳路由。**主区是 messenger**（左栏 + 时间线 + 输入区），聊天与工作目录一体。设置以分类形式存在，包含工具、记忆、渠道、定时、设备、归档等类目。

**舵机**是 ratatui TUI（`Viewport::Inline`），上方对话区、下方输入区；slash 命令 22 条（`slash_command.rs:183-208`），键位表权威见 `tui/app.rs:2241-2280`。

**舰桥**是治理与调试界面，密集、可扫描；顶层导航 **4 组 19 项**（`web/index.html:35/60/93/126`）：
- 系统 4：内部状态、系统设置、单位管理、用户管理
- 智能体 6：模型配置、线程管理、预设智能体、形象管理、工具管理、系统提示词
- 调试 6：吞吐量、性能、能力评估、多模调试、调试面板、LSP
- 文档 2：使用说明书、接口

工具管理导航项把 builtin / mcp / knowledge / skills / toolVisibility 归为同一组（`web/app.js:196` `TOOL_MANAGER_PANELS`）。调试面板右侧原日志区已重构为**线程轨迹视图**（`web/modules/log.js`、`trajectory-view.js`）。

## 能力矩阵（用户视角）

表格给出「有什么、没什么」的快速对照；每格来源以基线 H6 实测为准，具体实现细节请看各形态专页。

| 能力 | 蜂窝 Slint | 蜂巢 web | 舰桥 web/ | 舵机 TUI |
| --- | --- | --- | --- | --- |
| 线程列表 + 时间线 | ✅ | ✅ | ✅（线程管理） | ✅ |
| 归档 | ✅ 设置内类目 | ✅ | 未证实 | ✅ `/resume` |
| 工具 / MCP / 技能面板 | ✅ ToolsDialog | ✅ 设置工具类目（tab builtin/mcp/skill/knowledge） | ✅ 工具管理组 | ✅ `tool` / `mcp` / `skills` 子命令 |
| 知识库 | ✅ | ✅ | ✅ | ❌ 未实现 |
| 长期记忆 | ✅ 设置 + 专家数据卡 | ✅ | ❌ 未实现 | ❌ 未实现 |
| 定时任务 | ✅ | ✅ | ⚠️ 仅「预设智能体」模板 | ❌ 且被显式关闭 |
| 渠道绑定 | ✅ | ✅ | ⚠️ 仅「预设智能体」模板 | ❌ 未实现 |
| 形象系统 | ✅ + 原生浮窗（Win32） | ✅ | ✅ | ❌ 未实现 |
| 工作区文件 | ✅ | ✅ | ✅ | ⚠️ 仅 `!` shell 与 `@` 提及；`cloud ws ls/cat/pull/push` |
| 交互式终端 | ✅（winpty） | ❌ 未实现 | ❌ 未实现 | 不适用（宿主即终端） |
| 设备互通 | ⚠️ 侧栏云端工作区 + 审批弹窗；**设置类目未渲染** | ✅ 设置设备类目 + 互通视图 | ❌ 未实现（后端 API only） | ✅ `cloud devices/send/watch/audit/approve` |
| 语音输入 / TTS | 输入 ✅ / **TTS ❌** | 双向 ✅（含 TTS） | ❌ | 无 ASR（仅 Ctrl+V / 拖拽贴图） |
| 截图采集 | ✅ | ❌ | ❌ | ❌ |
| 托盘 / 系统通知 / 全局热键 | ✅ | ❌ | ❌ | ❌ |

## 按场景选择

- **个人用户日常使用**：装[蜂窝](/docs/zh-CN/start/desktop/)。本地文件、桌面控制、终端、截图、语音输入与托盘都在。
- **团队成员协作**：舰体部署后用浏览器访问[蜂巢](/docs/zh-CN/surfaces/frontend/)（云端 AI 工作台），nginx 容器 18001 端口下发 dist。
- **终端与自动化**：用[舵机](/docs/zh-CN/start/cli/)，`exec --json` JSONL 契约、`cloud` 子命令、脚本集成。
- **管理员治理**：舰桥。见[舰桥界面](/docs/zh-CN/surfaces/web-admin/)。

## 常见误区

- **舰桥能聊天吗？** 不能。它是治理与调试面板，普通用户登录后不会看到。
- **蜂巢与舰桥共用一套界面？** 不共用。蜂巢面向使用者，舰桥面向治理者，路由与视觉规范都不同。
- **蜂窝有「设备互通」设置面板？** 没有。设置页 12 分类里不包含互通类目；蜂窝侧真正能用的互通面只有两处：侧栏「工作目录区」的**云端工作区**（`ui/workspace_files.slint:99` 渲染 `wf_*` 键）+ 远端命令**审批弹窗**（`ui/main.slint:1460-1491`）。设置页里那套 `interlink-*` 属性与 `stc_interlink` / `il_*` 文案有代码但**零渲染引用**，当前入口未开放。
- **蜂窝有 TTS 播放？** 没有。语音**输入**已实现（`terminal_view.slint` + winpty 与 `src/audio_recording.rs`），设置里可以配置 TTS 模型但**没有播放调用**。
- **舵机能建定时任务？** 不能。`crates/wunder-cli/runtime.rs:683` 显式设置 `config.cron.enabled = false`；渠道、知识库、记忆与形象也没有管理界面，只在工具卡文案分类表里出现。
- **舰桥有用户级定时任务、渠道管理、长期记忆、互通面板与 goal UI？** 都没有。前两项仅作为「预设智能体」里的模板存在（`web/index.html:614/724-730`）；后三项在 `web/modules/state.js` 是死条目或没有实现。

## 延伸阅读

- [蜂窝界面](/docs/zh-CN/surfaces/desktop-ui/)
- [蜂巢界面](/docs/zh-CN/surfaces/frontend/)
- [舰桥界面](/docs/zh-CN/surfaces/web-admin/)
- [舵机使用](/docs/zh-CN/start/cli/)
- [核心概览](/docs/zh-CN/concepts/)
