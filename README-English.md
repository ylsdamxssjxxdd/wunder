# wunder Xinjian

wunder (Xinjian) is an **agent orchestration platform**: you hand a goal to the agent, it breaks the goal into tasks, calls tools to execute them step by step, and delivers results — files written into the workspace, commands executed, code changed, messages sent. The whole process is visible as an event stream, high-risk actions go through approval, and interrupted sessions can be restored.

It is not a question-and-answer AI. A Q&A AI produces text; wunder produces finished work.

## Get the Honeycomb (desktop) — install and go

**The Honeycomb (desktop)** is wunder's desktop app and its flagship form: a **local AI workbench**. Individual users just download and install it — no server deployment required:

1. **Download & install**: grab the installer for your system from [Releases](../../releases) and launch it
2. **Configure a model**: open "System Settings" → "Model Configuration", enter your API key and endpoint, then click "Test Connection"
3. **Start a conversation**: type a goal such as `list the files in the current directory and generate an inventory`, then press Enter

The agent starts planning, calling tools, and delivering results. See the [Honeycomb quick start](docs/使用说明书/zh-CN/start/desktop.md) for the full walkthrough.

The Honeycomb runs locally and shares the same Engine (wunder-runtime) and Core (wunder-core) as every other form. It can operate local files, windows, and the browser, stores data in SQLite, supports Windows 7 x86 and up, and ships a Linux AppImage.

## What it can do

- **Files & code**: read files, edit code, run commands, refactor projects
- **Office automation**: organize documents, generate reports, process spreadsheets, take meeting notes
- **Task splitting**: for parallel work, the agent temporarily spawns sub-agents and collects their results
- **Ongoing tasks**: scheduled inspections, periodic reminders, cross-channel message handling
- **Tool ecosystem**: built-in toolchain + MCP external tools + Skills for repeatable workflows + knowledge base
- **Long sessions**: context compaction and long-term memory keep long sessions working

## Basic Concepts

```text
Xinjian (the wunder platform)
├─ User
│  └─ Agent
│     └─ Thread
└─ ...
```

- User: the top-level principal for isolation and resource ownership; each user has exactly one agent instance
- Agent: the role that does the work — planning, calling models, calling tools
- Workspace: the directory where the agent works; tools are rooted there. In the Honeycomb it is a local directory with direct access to local files; in Hull deployments it is a server-side user directory
- Thread: the continuous execution context inside an agent
- Sub-agent: a temporary unit spawned by the agent during a task; it is reclaimed when the task ends

A goal lands on a user and is executed by the agent in its threads; when work needs to be split, temporary sub-agents are spawned.

## Advanced: other forms

Beyond the Honeycomb, wunder ships three optional forms — use them when you need them:

**The Hull (server) — the user and agent management platform.** Deploy it only when multiple people need to share one system. It provides multi-tenancy, user and organization management, agent app building and publishing, unified gateway access and scheduling, with a built-in toolchain, knowledge base, and long-term memory, PostgreSQL storage, and Docker deployment. Once deployed it provides two UIs: the admin frontend (the Bridge) and the Beehive (web, the cloud AI workbench, accessed in a browser). See the [Hull deployment guide](docs/使用说明书/zh-CN/start/server.md).

**The Helm (cli) — the manual local entry point.** For hands-on work in a terminal, scripting, and automation, sharing the same Engine as the Honeycomb. TUI interaction, JSONL output for scripts and pipelines. See the [Helm guide](docs/使用说明书/zh-CN/start/cli.md).

All forms share the same Engine (wunder-runtime): threads, tools, storage abstractions, realtime events, and permission semantics are one codebase — only the access layer differs.

## Tech Stack

| | Honeycomb (desktop) | Hull (server) | Helm (cli) |
| :--- | :--- | :--- | :--- |
| Backend | Rust, reuses the Engine (wunder-runtime), native in-process calls | Rust + axum 0.8 | Rust, reuses the Engine (wunder-runtime) |
| UI | Slint 1.18 native UI, software rendering | Vue 3 + TypeScript (built with Vite); the Bridge is plain HTML + JS (web/) | Terminal TUI (ratatui + crossterm) |
| Database | SQLite (rusqlite) | PostgreSQL | SQLite (rusqlite) |
| Access | In-process calls, no local networking | HTTP / WebSocket | Local process |
| Compatibility target | Windows 7 x86 and up, plus Linux AppImage | Linux / Docker | Windows 7 and up (built with the GNU toolchain) |

The backend is a single Rust workspace (tokio async runtime): `wunder-core` (configuration, auth, storage contracts, execution policy) → `wunder-runtime` (orchestrator, thread runtime, tools, channels, gateway). All forms are built on the Engine; the Honeycomb and the Helm do not depend on the Hull. The Helm parses arguments with clap; TLS uses rustls (ring provider).

## Documentation

- User manual (users / admins / developers): `docs/使用说明书/zh-CN/index.md`
- System introduction: `docs/wunder系统简介.md`
- API documentation: `docs/API文档.md`

## Projects Absorbed

wunder has absorbed code and ideas from many open-source projects along the way:

| Taken from | Project | Link |
| :--- | :--- | :--- |
| Project prototype | EVA | https://github.com/ylsdamxssjxxdd/eva |
| Agent foundation | OpenAI Codex | https://github.com/openai/codex |
| Frontend foundation | HuLa | https://github.com/HuLaSpark/HuLa |
| Protocol foundation | Claude Code | https://github.com/anthropics/claude-code |
| User foundation | OpenClaw | https://github.com/openclaw/openclaw |
