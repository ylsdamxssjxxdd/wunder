# wunder Xinjian

wunder (Xinjian) is an **agent orchestration platform**

## Form Matrix

| Icon | Form | Tech Stack | Role | How It Runs | Platforms |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 🚀 | Hull server | Rust + axum 0.8; PostgreSQL | User and agent management platform (multi-tenancy, org governance, unified gateway access and scheduling) | Server daemon, Docker deployment, HTTP / WebSocket access | Linux / Docker |
| 🐉 | Bridge web | Plain HTML + CSS + JavaScript (`web/`, no build, served by the Hull) | Admin frontend of the Hull (governance console: users, org units, channels, monitoring, evaluation) | Browser access, statically served by the Hull | Cross-platform browsers |
| 🐝 | Beehive web | Vue 3 + TypeScript (built with Vite) | User frontend of the Hull (cloud AI workbench) | Browser access, statically served by the Hull | Cross-platform browsers |
| 🍯 | Honeycomb desktop | Rust + Slint 1.18 (native UI, software rendering); SQLite | Desktop local AI workbench (flagship form, install and go) | Local desktop process, in-process engine calls (no local networking) | Windows 7 x86 and up; Linux AppImage |
| ⚙️ | Helm cli | Rust terminal TUI (ratatui + crossterm); SQLite | Local command-line entry point (hands-on work, scripting, automation) | Local terminal process, JSONL output supported | Windows 7 and up (built with the GNU toolchain) |

The Hull, Bridge, Beehive, Honeycomb, and Helm share one Engine (wunder-runtime): threads, tools, storage abstractions, realtime events, and permission semantics are a single codebase — only the access layer differs.

## Tool Matrix

| Tool | Description | Category |
| :--- | :--- | :--- |
| Plan panel `plan` | Push a structured task plan panel to the frontend | Wrap-up & frontend |
| Question panel `question_panel` | Show structured options, ask the user, and wait for a choice | Wrap-up & frontend |
| Session yield `sessions_yield` | Yield the current turn's control and be woken later | Wrap-up & frontend |
| List files `list` | List directory contents | Workspace & code |
| glob | Find file paths by glob pattern | Workspace & code |
| Search content `search` | Search file contents by regex | Workspace & code |
| Read file `read` | Read file contents, with range reads | Workspace & code |
| Write file `write` | Write or overwrite a file | Workspace & code |
| Text edit `edit_file` | Make exact string replacements in a file | Workspace & code |
| Exec `exec` | Run shell / system commands | Workspace & code |
| Command session `command_session` | Manage long-lived background command sessions with foreground/background switching | Workspace & code |
| Skill call `skill_call` | Invoke an installed skill | Workspace & code |
| Read image `read_image` | Read image content for the model to understand | Multimodal |
| Generate image `generate_image` | Generate an image from a prompt | Multimodal |
| Generate video `generate_video` | Generate a video from a prompt | Multimodal |
| Generate speech `generate_speech` | Text to speech | Multimodal |
| Transcribe speech `transcribe_speech` | Speech to text | Multimodal |
| Web fetch `web_fetch` | Fetch and parse web page content | Web & desktop |
| Browser `browser` | Drive a browser to click, type, wait for rendering, screenshot, etc. | Web & desktop |
| Desktop monitor `desktop_monitor` | Read desktop window and screen state | Web & desktop |
| Desktop controller `desktop_controller` | Operate the local desktop (mouse, keyboard, windows) | Web & desktop |
| Subagent control `subagent_control` | Spawn temporary sub-agents during a task and reclaim them after results return | Threads & sub-agents |
| Thread control `thread_control` | Manage task threads / forked threads | Threads & sub-agents |
| Self status `self_status` | Query the agent's own runtime status | System & memory |
| Memory manager `memory_manager` | Read and write long-term memory | System & memory |
| User world `user_world` | Access user and organization world info | System & memory |
| Schedule task `schedule_task` | Create and manage scheduled / periodic tasks | System & memory |

## Absorbed Matrix

| Taken from | Project | Link |
| :--- | :--- | :--- |
| Project prototype | EVA | https://github.com/ylsdamxssjxxdd/eva |
| Agent foundation | OpenAI Codex | https://github.com/openai/codex |
| Frontend foundation | HuLa | https://github.com/HuLaSpark/HuLa |
| Protocol foundation | Claude Code | https://github.com/anthropics/claude-code |
| User foundation | OpenClaw | https://github.com/openclaw/openclaw |
| Tool foundation | deepseek-harness (dsh) | https://github.com/deepseek-ai/deepseek-harness |