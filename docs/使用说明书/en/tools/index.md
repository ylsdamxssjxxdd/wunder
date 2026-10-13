---
title: Tools Overview
summary: wunder's built-in tool matrix, eight categories pointing at the matching tool pages, plus selection entry points by situation, the conventions for how results are shaped, and a few exceptions that skip the shared shell.
updated_at: 2026-10-13
---

wunder's tool system covers files, commands, the web, the desktop, collaboration, and system connections. Most built-in tools converge on one shared shell for both success and failure; a few stay outside it.

## The tool matrix

Most canonical tool names the model sees are Chinese, each paired with an English alias, normalized by `resolve_tool_name` (`crates/wunder-runtime/src/services/tools/catalog.rs`). MCP tools (`server@tool`), skill names, knowledge-base names, and user tool aliases are registered dynamically from configuration and are not in the table below.

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

`网页搜索` and `ptc` are retained in code but currently **temporarily delisted** — invisible to the model and to the user (see `default_tool_profile.rs`); they are not removed. Removed entirely: channel tools, user-world tools, `a2a_*` tools, `final_response`, `a2ui`, LSP query tools.

## Choose a tool by situation

### You only have keywords and want a web source

Web search is currently delisted; when you already have a URL, read the page directly with [Web Fetch](/docs/en/tools/web-fetch/).

### You only want to read the main content of a page

Start with [Web Fetch](/docs/en/tools/web-fetch/); do not jump straight to the browser.

### You need to click, type, or wait for dynamic rendering

Use [Browser](/docs/en/tools/browser/).

### You want to look at local code

The usual order is: list directories or search with [Workspace Files](/docs/en/tools/workspace-files/) first, then read the ranges you need.

### You want to change code

- Small, precise, context-matched edits: use the patch form of `文本编辑`
- One exact text replacement: use `文本编辑` in [Workspace Files](/docs/en/tools/workspace-files/)
- Complex replacement logic: run a script with [Execute Command](/docs/en/tools/exec/)
- Rewriting a whole file: use `write_file` in [Workspace Files](/docs/en/tools/workspace-files/)
- Compiling, testing, or running a script: use [Execute Command](/docs/en/tools/exec/)

### You want to collaborate

- Spinning up a temporary child run in the current session: use [Subagent Control](/docs/en/tools/subagent-control/)
- Managing the task thread or branch threads: use [Thread Control](/docs/en/tools/thread-control/)

## Docs by category

### Turn finishing and frontend coordination

- Plan Panel / Question Panel / Sessions Yield: [Plan Panel / Question Panel / Sessions Yield](/docs/en/tools/panels/)

### Workspace and code

- List files / glob / search content / read file / write file / text edit: [Workspace Files](/docs/en/tools/workspace-files/)
- Execute command / command session: [Execute Command](/docs/en/tools/exec/)
- Skill call: [Skill Call](/docs/en/tools/skill-call/)

### Web

- Web fetch: [Web Fetch](/docs/en/tools/web-fetch/)
- Browser: [Browser Tools](/docs/en/tools/browser/)

### Multimodal

- Read image: [Read Image](/docs/en/tools/read-image/)
- Generate image / generate video / generate speech / transcribe speech: canonical names `图像生成` / `视频生成` / `语音生成` / `声转文`, aliases `generate_image` / `generate_video` / `generate_speech` / `transcribe_speech`; no dedicated page in this section.

### Desktop

- Desktop monitor / desktop controller: [Desktop Control](/docs/en/tools/desktop-control/)

### Threads and subagents

- Subagent control: [Subagent Control](/docs/en/tools/subagent-control/)
- Thread control: [Thread Control](/docs/en/tools/thread-control/)

### System and memory

- Self status: [Self Status](/docs/en/tools/self-status/)
- Memory manager: [Memory Manager](/docs/en/tools/memory-manager/)
- Schedule task: [Schedule Task](/docs/en/tools/schedule-task/)

### Goal state

- `get_goal` / `create_goal` / `update_goal`: injected by the runtime per session, and not offered for selection on the desktop endpoint.

## How to read results

Most built-in tools return one shared shell on success: whether it succeeded, the action name, a state, a one-line summary, and a data area that carries the business result. Read the business result from the data area first. Some tools add one more line telling you what to do next.

Common states include: completed, dry run only, accepted, running, yielded, awaiting input, partially complete, timed out, and no change. A timeout does not always mean a permanent failure.

Failures use the same shared shell: an error line, a readable reason and fix hint, and data that helps with diagnosis.

A few exceptions do not use the shared shell:

- **Final Response** (removed): only a termination signal, returning a single answer to the user.
- **Schedule Task**: returns a compact scheduling result.
- **Browser tool group**: results use a content-block structure (see [Browser](/docs/en/tools/browser/)).
- **Web Search** (delisted): used to search the public web by keyword, with results in the data area's `results` field; to read a page now, use Web Fetch directly.
- **Web Fetch**: returns the fetched result object directly on success.
- **Text Edit** (formerly `apply_patch`): success uses the shared shell; in its patch form, failures carry stricter error semantics than ordinary file tools.

## See also

- How to read and write files and code → [Workspace Files](/docs/en/tools/workspace-files/)
- How subagents are spawned and reclaimed → [Subagent Control](/docs/en/tools/subagent-control/)
- What one specific tool returns → go to that tool's page
