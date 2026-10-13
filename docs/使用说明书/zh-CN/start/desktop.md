---
title: 蜂窝（desktop）入门
summary: wunder 的默认使用方式：本地 AI 工作台。下载安装即可使用，不需要先部署舰体；同进程原生调用引擎，无端口，无 HTTP/WebSocket。
read_when:
  - 第一次装蜂窝，需要装、启动、验证的完整路径
  - 想知道蜂窝在本地存了什么、能操作什么
  - 需要判断「蜂窝 vs 舰体」
source_docs:
  - frontend-slint/Cargo.toml
  - frontend-slint/run.bat
  - frontend-slint/src/main.rs
  - crates/wunder-desktop/src/args.rs
  - crates/wunder-desktop/src/native.rs
  - crates/wunder-desktop/src/runtime.rs
updated_at: 2026-10-13
---

# 蜂窝（desktop）入门

蜂窝（desktop）是 wunder 的桌面版，也是**默认使用方式**：一个**本地 AI 工作台**。下载安装即可使用，不需要先部署舰体（server）；同进程原生调用引擎（wunder-runtime），无独立端口、无 HTTP/WebSocket 中转。

## 适用场景

| 场景 | 选蜂窝 |
| --- | --- |
| 想直接开始用，不想先部署服务 | ✅ |
| 需要操作本地文件、窗口、浏览器 | ✅ |
| 更习惯图形界面 | ✅ |
| 需要交互式终端与截图采集 | ✅ |
| 需要托盘、系统通知与全局热键 | ✅ |
| 后面可能接远端，但现在先跑起来 | ✅ |
| 需要多用户治理与管理员后台 | ❌ 选舰体（server） |
| 需要在浏览器里访问 | ❌ 选蜂巢（web，随舰体） |

## 装与启动

- **可执行文件**：`wunder-frontend-slint`（`frontend-slint/Cargo.toml:2`，默认入口 `src/main.rs`）
- **开发启动**：`frontend-slint/run.bat`（Windows）
- **平台**：主要面向 32 位 Win7 与 Ubuntu 18.04；现代 Windows / Linux 均可运行
- **无端口、无必需参数**：`NativeDesktop::start_with_args` 直接同进程调引擎（`src/main.rs:102-108`）
- **native_defaults**：`port=0, bridge_only=true, native_runtime=true`（`crates/wunder-desktop/src/args.rs:53-67`）
- 独立 bridge 仅面向既有兼容客户端（默认 18123，`crates/wunder-desktop/src/bridge.rs:67`）；日常使用不走这条路
- Electron / Tauri 前端壳**停止日常维护与功能迭代**（不代表蜂窝后端停止维护）

## 本地数据

| 内容 | 位置 |
| --- | --- |
| 根目录 | `WUNDER_HOME` → 默认 `~/.wunder`（`crates/wunder-desktop/src/runtime.rs:509-521`） |
| 工作区 | `~/.wunder/workspace`（`runtime.rs:469-505`） |
| 临时目录 | `WUNDER_DESKTOP_TEMP_ROOT`（`runtime.rs:451-466`） |
| 桌面设置 | `<temp>/config/desktop.settings.json`（`runtime.rs:150`） |
| 会话与工具日志 | 本地 SQLite |

迁移到新机器：复制 `~/.wunder`；启动时会做工作树扫描（`desktop_runtime_recovery.rs:12-45`，限 4096），恢复 queued/waiting/running/cancelling 状态并清理会话锁与残留任务。

## 界面结构（主窗只有两页）

蜂窝的导航枚举只有 **messages / settings**（`ui/theme.slint:144`），其余能力以面板与对话框挂载。

### 对话页（messages）

- **左栏**：任务与线程树、**工作目录区**（本地文件树 + 用量条 + **云端工作区**）、底部设置入口
- **右栏**：顶栏（工作区 / 线程名）+ 时间线 + 输入区
- 未选中线程时显示欢迎页；文件预览、在线编辑等以浮层弹出，不改变两栏结构
- 一个智能体可以并行多个**独立任务线程**，工作区可以是**多个**：本地 + 云端 + 通过设备互通访问的远程设备工作区

### 设置页（settings）

**12 个分类**（`ui/settings_page.slint:672-750`）：

通用、账户、模型、工具、智能体、形象、定时、记忆、渠道、提示词包、归档、帮助。

分类导航支持搜索过滤。**没有「设备互通」类目**。

## 5 步上手

### 1. 下载安装

从 Releases 下载对应系统的安装包。

### 2. 首次启动

启动后先显示轻量启动页，再准备本地工作目录、配置与会话。窗口出现不代表会话已就绪；进入工作台后即可操作。首次启动或设备较慢时准备时间可能更长。

### 3. 配置模型

设置 → 模型 → 添加：

- API Key
- 接口地址（`https://example.com/v1` 这类中性端点）
- 模型名称与参数（temperature、max_context 等）

保存前用「测试连接」确认。想接入 TTS / 图像 / 视频模型也在这里；**TTS 当前只能配置，无播放实现**。

### 4. 完善账号资料

设置 → 账户：用户名、邮箱、头像；如需长期使用请设置登录密码。

### 5. 发起第一次对话

回到对话页，输入：

```
帮我列出当前工作目录的文件
```

看到模型思考 → 工具批次 → 最终回复落到聊天区，就说明跑通。

## 蜂窝独有能力（其他界面没有）

- **交互式终端**（`ui/terminal_view.slint:191` + winpty / PTY）
- **截图采集**（`src/screenshot.rs` + overlay，XGetImage / Win32）
- **托盘与系统通知**（`tray.slint`、`tray-notice.slint`）
- **全局热键**（`src/hotkey.rs`）
- **语音输入**（`native.rs:846,854` + `src/audio_recording.rs` + `native_chat.rs:1722,1795`）

## 关于设备互通

蜂窝里能用的互通面**只有两处**：

1. 侧栏「工作目录区」的**云端工作区**：走 `ui/workspace_files.slint:99` 的 `I18n.wf_offline` / `wf_connect` 等 `wf_*` 键，网络不可用时明确显示离线并给出「连接」按钮
2. **远端命令审批弹窗**（`ui/main.slint:1460-1491`）：互通 L3 命令的高危确认，danger 边框、level/risk/from 元信息与「不允许记忆」提示

设置页里那套 `interlink-*` 属性 / 回调（`settings_page.slint:505-538`）与 `stc_interlink` / `il_*` i18n 键在 `ui/*.slint` 与 `src/*.rs` 中**零渲染引用**，属「实现了但没接线」，**当前入口未开放**。

## 一键重置工作状态

会话、子智能体或工作区状态混乱时，用「设置 → 通用 → 一键重置工作状态」：

- **会做**：停止运行中的会话、清空排队任务、清理会话锁
- **保留**：技能、知识库、长期记忆、用户配置、工作区文件、会话历史

它是**清理工作状态**，不是「恢复出厂设置」。

## 两个交互约束

- **会话队列（默认开）**：`agent_queue.enabled` 默认 true；关闭时运行中再发消息会被判「用户忙」且**不进排队条**。保持默认即可。
- **子智能体运行时的提示**：任务派生了子智能体时，聊天区会显示仍在工作的子运行状态；子智能体是主智能体临时创建的工作单元，任务结束即回收。

## 常见问题

**数据在哪里？**
工作区在 `~/.wunder/workspace`，会话历史在本地 SQLite，配置在 `<temp>/config/desktop.settings.json`。

**能迁到别的电脑吗？**
能。复制 `~/.wunder` 到新机器即可。

**本地模式能联网吗？**
可以。「本地优先」只指数据留本地，不影响访问远程模型或网页。

**能多人共用吗？**
蜂窝专注本地单人场景；团队协作请部署舰体，成员用蜂巢。

## 延伸阅读

- [快速开始](/docs/zh-CN/start/quickstart/)
- [蜂窝界面](/docs/zh-CN/surfaces/desktop-ui/)
- [界面概览](/docs/zh-CN/surfaces/index/)
- [首次运行排查](/docs/zh-CN/start/troubleshooting-first-run/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
