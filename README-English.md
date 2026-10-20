# wunder (Xinjian)

wunder is an **agent orchestration platform**:

Diverse (multi-platform, multi-form factor), Fast (fast startup, rapid response), Superior (robust stability, high intelligence), Efficient (low CPU & memory footprint, low context overhead)

## Form Matrix

| Icon | Form | Tech Stack | Role | How It Runs | Platforms |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 🚀 | Hull (server) | Rust + axum 0.8; PostgreSQL | User-and-agent management platform. All users share a single sandbox with no barriers between them | Server daemon, Docker deployment | Linux / Docker |
| 🐉 | Bridge (web) | Plain HTML + CSS + JavaScript (`web/`, no build) | Hull's governance console (four nav groups: system, agents, debugging, docs) | Browser access, statically mounted by the Hull at `/` | Cross-platform browsers |
| 🐝 | Beehive (web) | Vue 3 + TypeScript (Vite build, `frontend/`) | Cloud AI workbench (the Hull's user frontend) | Browser access; the built `frontend/dist` is mounted read-only into an nginx container and served externally (default port 18001) — **not hosted by the Hull** | Cross-platform browsers |
| 🍯 | Pupa (desktop) | Rust + Slint 1.18 (default frontend `frontend-slint/`, native UI, software rendering); SQLite | Local AI workbench (flagship form, install and go) | Local desktop process calling the engine in-process (no local HTTP); Electron / Tauri shells are no longer maintained | Windows 7 x86 and up; Linux (ubuntu18.04 baseline) x64 / arm64 AppImage |
| ⚙️ | Helm (cli) | Rust terminal TUI (ratatui + crossterm); SQLite | Hands-on local entry point (terminal work, scripting, automation) | Local terminal process calling the engine in-process; JSONL output supported | Windows 7 and up (GNU toolchain); Linux amd64 / arm64 (ubuntu18.04 sysroot) |

All forms share one engine (wunder-runtime) and one core (wunder-core): threads, tools, storage abstractions, realtime events, and permission semantics are a single codebase — only the access layer differs. Pupa and Helm run standalone without the Hull.

One agent can own multiple independent task threads at the same time; scheduling, locks, and live status are isolated per thread. Workspaces can coexist as a local directory, the cloud directory `/workspaces/<user>/workspace`, and remote-device workspaces.

## Performance Matrix

Performance characteristics of the same engine across forms. Values are to be filled in after measurement; sampling rules and regression thresholds live in [性能标准](docs/性能标准.md), scoring rules in [性能评分体系](docs/性能评分体系.md) (each form scored independently — do not sum across forms), and historical data in [性能基线](docs/性能基线/).

| Icon | Form | Startup speed | Memory usage | CPU usage | Package size | Concurrent agent threads | Chat page performance |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| 🚀 | Hull (server) | — | — | — | — | — | — |
| 🐉 | Bridge (web) | — | — | — | — | — | — |
| 🐝 | Beehive (web) | — | — | — | — | — | — |
| 🍯 | Pupa (desktop) | — | — | — | — | — | — |
| ⚙️ | Helm (cli) | — | — | — | — | — | — |

## Tool Matrix

Canonical tool names are mostly Chinese (what the model sees), each with English aliases normalized via `resolve_tool_name` (`crates/wunder-runtime/src/services/tools/catalog.rs`). MCP tools (`server@tool`), skill names, knowledge-base names, and user-tool aliases are registered dynamically and are not listed here.

| Tool (canonical) | English alias | Description | Category |
| :--- | :--- | :--- | :--- |
| 计划面板 | `update_plan` | Push a structured task plan panel to the frontend | Wrap-up & frontend |
| 问询面板 | `question_panel` | Show structured options, ask the user, and wait for a choice | Wrap-up & frontend |
| 会话让出 | `sessions_yield` | Yield the current turn's control and be woken later | Wrap-up & frontend |
| 列出文件 | `list_files` | List directory contents | Workspace & code |
| glob | — | Find file paths by glob pattern | Workspace & code |
| 搜索内容 | `search_content` | Search file contents by regex | Workspace & code |
| 读取文件 | `read_file` | Read file contents, with range reads | Workspace & code |
| 写入文件 | `write_file` | Write or overwrite a file | Workspace & code |
| 文本编辑 | `edit` / `edit_file` | Exact replacement / patch / subcommand modes (`apply_patch` merged in) | Workspace & code |
| 执行命令 | `execute_command` | Run shell / system commands | Workspace & code |
| 命令会话 | `command_session` | Manage long-lived background command sessions with foreground/background switching | Workspace & code |
| 技能调用 | `skill_call` | Read a skill's body and run its scripts | Workspace & code |
| 网页抓取 | `web_fetch` | Fetch and parse web page content (requires enabling) | Web |
| 浏览器 | `browser` | One strict action tool for navigation, observation, interaction, waits, and screenshots | Web |
| 读图工具 | `read_image` | Read image content for the model to understand (requires vision) | Multimodal |
| 图像生成 | `generate_image` | Generate an image from a prompt | Multimodal |
| 视频生成 | `generate_video` | Generate a video from a prompt | Multimodal |
| 语音生成 | `generate_speech` | Text to speech | Multimodal |
| 声转文 | `transcribe_speech` | Speech to text | Multimodal |
| 桌面控制 | `desktop_control` | Operate and observe the local desktop (mouse, keyboard, windows) | Desktop |
| 子智能体控制 | `subagent_control` | Spawn temporary subagents during a task and reclaim them after results return | Threads & subagents |
| 会话线程控制 | `thread_control` | Manage task threads / forked threads (not exposed on desktop) | Threads & subagents |
| 自我状态 | `self_status` | Query the agent's own runtime status | System & memory |
| 记忆管理 | `memory_manager` | Read and write long-term memory | System & memory |
| 定时任务 | `schedule_task` | Create and manage scheduled / periodic tasks | System & memory |
| `goal` | `get / create / edit / pause / resume / complete / blocked` | Goal-state tool, auto-injected per session by the runtime; not selectable on desktop | Goal state |


## Absorbed Matrix

| Taken from | Project | Link |
| :--- | :--- | :--- |
| Project prototype | EVA | https://github.com/ylsdamxssjxxdd/eva |
| Agent foundation | OpenAI Codex | https://github.com/openai/codex |
| Frontend foundation | HuLa | https://github.com/HuLaSpark/HuLa |
| Protocol foundation | Claude Code | https://github.com/anthropics/claude-code |
| User foundation | OpenClaw | https://github.com/openclaw/openclaw |
| Tool foundation | deepseek-harness (dsh) | https://github.com/deepseek-ai/deepseek-harness |
