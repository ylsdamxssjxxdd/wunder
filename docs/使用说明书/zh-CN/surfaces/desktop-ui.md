---
title: 蜂窝界面
summary: 蜂窝（desktop）是本地 AI 工作台，主推应用。默认前端 Rust + Slint，主窗只有两页（对话、设置），设置内 12 分类；其他能力以面板与对话框呈现。
read_when:
  - 第一次打开蜂窝，想知道哪些区能点什么
  - 想搞清蜂窝与蜂巢 / 舵机的能力差异
  - 需要判断某项能力在蜂窝里到底能不能用
source_docs:
  - frontend-slint/ui/main.slint
  - frontend-slint/ui/theme.slint
  - frontend-slint/ui/settings_page.slint
  - frontend-slint/ui/workspace_files.slint
  - frontend-slint/ui/terminal_view.slint
  - crates/wunder-desktop/src/lib.rs
  - crates/wunder-desktop/src/native.rs
updated_at: 2026-10-13
---

# 蜂窝界面

蜂窝（desktop）是 wunder 的**本地 AI 工作台**，也是个人用户的默认入口与主推应用。它没有独立后端端口，也没有 HTTP/WS 中转：可执行 `wunder-frontend-slint` 直接同进程原生调用引擎（wunder-runtime）。

## 启动与运行

- 可执行名：`wunder-frontend-slint`（`frontend-slint/Cargo.toml:2`）
- 开发启动：`frontend-slint/run.bat`
- 无端口、无必需参数；默认路径 `wunder_desktop::NativeDesktop::start_with_args`（`frontend-slint/src/main.rs:102-108`），进入 `DesktopArgs::native_defaults()`：`port=0`、`bridge_only=true`、`native_runtime=true`（`crates/wunder-desktop/src/args.rs:53-67`）
- 数据目录：`WUNDER_HOME` → `~/.wunder`；工作区 `~/.wunder/workspace`；桌面设置 `<temp>/config/desktop.settings.json`
- 独立 bridge 仅面向既有兼容客户端（默认端口 18123），日常使用不走这条路

## 主窗：只有两页

`ui/theme.slint:144` 定义了导航枚举 `enum DesktopPage { messages, settings }`，主窗只有对话页与设置页两个可切换页面。其余能力以**面板与对话框**形式挂在两页之内。

### 对话页（messages）

左右两栏：

- **左栏**：任务与线程树、工作目录区、设置入口
  - 工作目录区渲染本地工作树 + **云端工作区**（`ui/workspace_files.slint:99` 的 `I18n.wf_offline` / `wf_connect` 等 `wf_*` 键真实生效）
  - 一个智能体可以同时拥有**多个独立任务线程**，工作区也可以有**多个**：本地目录 + 云端工作区 + 通过设备互通访问的远程设备工作区
- **右栏**：顶栏（工作区/线程名）+ 时间线 + 输入区
  - 未选中线程时显示欢迎页
  - 输入区含模型切换、审批模式、思考等级、附件、语音输入按钮（如可用）、截图按钮（如可用）、发送/停止
  - 每轮回复下方的「工作过程」条目折叠中间思考、工具批次与中间打印

### 设置页（settings）

**12 个分类**（导航 `ui/settings_page.slint:672-750`）：

| 顺序 | 分类 | 说明 |
| --- | --- | --- |
| 0 | 通用 | 界面语言、外观、启动行为 |
| 13 | 账户 | 用户名、邮箱、密码、头像 |
| 1 | 模型 | 添加/编辑 LLM、TTS、图像、视频模型；TTS 只允许配**不能播**（见下文） |
| 2 | 工具 | 打开 ToolsDialog，builtin / mcp / skills / knowledge 分组管理 |
| 3 | 智能体 | 系统提示词、默认审批模式、默认技能集 |
| 4 | 形象 | 形象包选择、浮窗 scale；仅 Win32 分层窗口，Linux 无等价浮窗 |
| 5 | 定时 | 蜂窝的定时任务入口（`ui/cron_page.slint` + `native_cron.rs`） |
| 6 | 记忆 | 长期记忆管理、命中记录 |
| 7 | 渠道 | 渠道绑定与状态（`ui/channels_page.slint` + `native_channels.rs`） |
| 9 | 提示词包 | 站内可用的 prompt pack 与语言默认 |
| 10 | 归档 | 归档线程列表 |
| 11 | 帮助 | 内嵌文档、版本号 |

分类导航条支持搜索过滤（`settings-category-flags` 位向量），无匹配时显示空态而不是留白。

## 面板与对话框（对话页与设置页挂载）

以下都是蜂窝里的**独立区域**，不是「页面」：

- **侧栏 / 时间线 / 输入区**：对话页三个组件
- **终端视图**（`ui/terminal_view.slint:191`）：交互式命令会话，Windows 下走 winpty feature；非 Windows 有等价 PTY
- **工具管理对话框 ToolsDialog**：builtin / mcp / skills / knowledge 分组；`src/tools_ui.rs:445` 起
- **形象对话框**：形象包列表 + 预览
- **工作区对话框 workspace_dialog**：切换/新建工作区
- **工作目录区 workspace_files**：文件树 + 工具栏 + 用量条 + 云端工作区
- **线程轨迹 thread_trajectory**：整线程执行台账（取代旧的日志对话框）
- **子智能体详情 subagent_detail**
- **渠道页 channels_page**：蜂窝用户面渠道管理
- **定时页 cron_page**：蜂窝用户面定时任务
- **活动卡 activity_card / 专家数据 expert_data / 补丁差异卡 patch_diff_card**
- **截图遮罩 screenshot_overlay**：全屏截图后回抛到输入区
- **托盘 tray.slint 与系统通知 tray-notice.slint**：后台运行、完成提醒
- **远端命令审批弹窗**（`ui/main.slint:1460-1491`）：互通 L3 命令的高危确认；有 danger 边框、level/risk/from 元信息与「不允许记忆」提示

## 蜂窝独有能力（其他界面没有）

| 能力 | 位置 |
| --- | --- |
| **交互式终端** | `terminal_view.slint` + winpty / PTY，蜂巢、舰桥、舵机都没有 |
| **截图采集** | `src/screenshot.rs` + overlay，XGetImage / Win32 |
| **托盘与系统通知** | `tray.slint`、`tray-notice.slint` |
| **全局热键** | `src/hotkey.rs` |
| **语音输入** | `native.rs:846,854` + `src/audio_recording.rs` + `native_chat.rs:1722,1795` |
| **本地文件与桌面控制** | 桌面工具（`桌面控制器` / `桌面监视器`），需 vision 能力 |

## 已知不可用或未接线（**手册不得写成可用**）

- **设置页「设备互通」类目未渲染**：`ui/settings_page.slint:505-538` 声明了 5 属性 + 5 回调，`main.slint:1146-1159` 有透传，Rust 侧 `src/native_interlink.rs:59-118` 也在填数据，`ui/i18n.slint` 里 `stc_interlink`、`il_tunnel_title`、`il_browse_title`、`il_devices_note` 等键都准备好了，但在 `ui/*.slint` 与 `src/*.rs` 里**引用数为 0**。属于「实现了但没接线」，当前入口未开放。蜂窝里能用的互通面只有两处：侧栏「工作目录区」的**云端工作区** + 远端命令**审批弹窗**。
- **TTS 未实现**：`native_pages.rs:78,1027` 只允许配置 tts 模型，**没有播放调用**，`native.rs:846,854` 与 `src/audio_recording.rs` 只做语音**输入**。想听到语音输出请用蜂巢（`ChatComposer.vue:299` 双向，TTS 在 `messengerControllerInstall.ts:9523`）。
- **Electron / Tauri 前端壳停止日常维护与功能迭代**：仓库内已无 `src-tauri` 或 `tauri.conf.json`，只有 `crates/wunder-desktop/README.md` 与 `scripts/build-win7.ps1` 等脚本里的死链。这不等于停止维护蜂窝后端——引擎照常运行。

## 一键重置工作状态

异常状态（会话卡在运行中、子智能体状态不一致、界面与实际不符）下，用「设置 → 通用 → 重置工作状态」：

- 会做：停止运行中的会话、清空排队任务、清理会话锁与状态
- 保留：技能、知识库、长期记忆、用户配置、工作区文件、会话历史

所以它是**清理工作状态**，不是「恢复出厂」。

## 蜂窝与另一端差异

| 项 | 蜂窝 | 蜂巢 |
| --- | --- | --- |
| 前端 | Rust + Slint（`frontend-slint/`） | Vue3 + TS（`frontend/`） |
| 后端 | 同进程原生调用引擎，无端口 | 舰体（默认 18000），nginx 18001 下发 dist |
| WebSocket | 无（同进程 façade） | 只有一条 `/chat/ws` 多路复用 |
| 交互式终端 | ✅ 有 | ❌ 无 |
| 语音输入 / TTS | 输入 ✅，TTS ❌ | 双向 ✅（含 TTS） |
| 托盘 / 系统通知 / 全局热键 / 截图 | ✅ | ❌ |
| 设备互通设置面板 | 类目未渲染；仅云端工作区 + 审批弹窗 | ✅ 设备类目 + `views/messenger/interlink/*` |
| 数据存放 | 本地 SQLite `~/.wunder` | 舰体存储（默认 PostgreSQL） |
| 一个智能体 | 多个线程，多个工作区 | 同 |

## 常见问题

**数据在哪里？**
工作区在 `~/.wunder/workspace`，会话与工具日志在本地 SQLite，配置在 `<temp>/config/desktop.settings.json`；`WUNDER_HOME` 可覆盖根。

**本地模式能联网吗？**
能。「本地优先」指数据留在本机，不影响访问远程模型或网页。

**能迁到别的电脑吗？**
能。复制 `~/.wunder`（含工作区、SQLite 与设置）到新机器；首次启动时会做工作树扫描（`desktop_runtime_recovery.rs:12-45`，限 4096），恢复 queued/waiting/running/cancelling 状态。

**运行中能不能新建线程？**
可以；一个智能体同时拥有多个独立任务线程，锁与实时状态按线程隔离，会话队列开启时新消息进入排队条而不是被判「用户忙」。关掉 `agent_queue.enabled` 才会导致该问题。

## 延伸阅读

- [快速开始](/docs/zh-CN/start/quickstart/)
- [蜂窝入门](/docs/zh-CN/start/desktop/)
- [界面概览与能力矩阵](/docs/zh-CN/surfaces/index/)
- [蜂巢界面](/docs/zh-CN/surfaces/frontend/)
- [首次运行排查](/docs/zh-CN/start/troubleshooting-first-run/)
