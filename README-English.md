# wunder Xinjian

An agent orchestration system. You give it a goal; it breaks the task down, calls tools, and delivers results.

The frontend is a warm hive, the backend is a massive starship.

## Current Status

The project is in prototype stage. APIs and structure may change at any time.

The three runtime forms are at different stages:

- **server**: the core of the project, most complete, and the main focus of daily development
- **desktop**: under development, not feature-complete yet; a local desktop app built on Slint, intended as the main form for individual users
- **cli**: under development, not feature-complete yet; a command-line form sharing the server kernel

For a full experience today, start with server. Desktop and cli run, but don't expect them to cover everything server does.

## Core Concepts

```text
Xinjian (wunder platform)
├─ User
│  └─ Swarm
│     └─ Agent
│        └─ Thread
└─ ...
```

- User: the top-level boundary for isolation and resource ownership
- Swarm: a group of agents organized around one goal
- Agent: the role that does the work — planning, calling models, calling tools
- Thread: the continuous execution context inside an agent

A goal lands on a user, gets divided inside a swarm, and is executed by agents in their own threads.

## Three Runtime Forms

**wunder-server** is the core. Multi-user, multi-tenant, agent app building and publishing, unified gateway access, with a built-in toolchain, knowledge base, and long-term memory. Deploy on Linux for best performance.

**wunder-desktop** (in development) is the local desktop form. The default frontend is Rust + Slint; on launch it calls the backend natively in the same process, with no local HTTP involved. The compatibility target is 32-bit Windows 7.

**wunder-cli** (in development) is the command-line form. It reuses the runtime core instead of building a separate execution chain.

All three forms share one core: threads, tools, storage, realtime events, and permission semantics are the same code; only the access layers differ.

## Tech Stack

The backend is a single Rust workspace (tokio async runtime) across all forms:

| | server | desktop | cli |
| :--- | :--- | :--- | :--- |
| Backend | Rust + axum 0.8 | Rust, reuses the runtime core, native in-process calls | Rust, reuses the runtime core |
| UI | Vue 3 + TypeScript (built with Vite) | Slint 1.18 native UI, software rendering | Terminal TUI (ratatui + crossterm) |
| Admin console | Plain HTML + JS (web/) | — | — |
| Database | PostgreSQL | SQLite (rusqlite) | SQLite (rusqlite) |
| Access | HTTP / WebSocket | In-process calls, no local networking | Local process |
| Compatibility target | Linux / Docker | Windows 7 x86 and up, plus Linux AppImage | Windows 7 and up (built with the GNU toolchain) |

CLI argument parsing uses clap; TLS uses rustls (ring provider). Desktop uses the native Slint frontend and shared Rust runtime.

## Capabilities

- Multi-agent parallel collaboration and task handoff
- MCP tool integration, Skills for repeatable workflows
- Scheduled tasks, gateway, and multi-channel access
- Context compaction and long-term memory for long sessions
- Knowledge base

## Ten Cores

| Core | Design Goal | Core Capabilities |
| --- | --- | --- |
| Goal | Let agents enter a goal state and continuously work one task through to completion | Goal-state entry and sustainment, continuous execution, progress tracking, completion check |
| Compatibility | Keep the desktop and command-line forms explicitly constrained and runnable on legacy operating systems | Windows 7 and Ubuntu 18.04 adaptation, dual desktop/CLI forms, runtime version pinning, dependency downgrade, startup self-check |
| Context Compaction | Control context size in long sessions while retaining useful information | Manual compaction, automatic compaction, overflow recovery, compaction summary re-injection, before/after comparison and replay |
| Sub-agents | Derive subtasks within a single thread, completed by independent executors that return results | Subtask breakdown, sub-agent creation and reclamation, result aggregation, parent-child session linking |
| Scheduled Tasks | Support system-level periodic execution and background governance | Scheduled triggers, planned tasks, background inspection, automatic maintenance, async execution chains |
| Channel Communication | Support multiple entry points into the same runtime capabilities | HTTP, WebSocket, Desktop, CLI, third-party channels, gateway adaptation |
| Memory | Support long sessions and long-term material reuse without polluting the thread's core cognition | Thread-init memory injection, knowledge base, workspace files, long-term material reading |
| Swarm | Support a mother bee dispatching multiple worker bees to complete collaborative tasks | Task breakdown, worker dispatch, node state sync, result aggregation, parent-child session linking |
| Multithreading | Improve throughput and resource utilization through concurrency control within a single process | Worker thread pool, concurrent task scheduling, thread isolation, resource caps, race and deadlock protection |
| Multi-tenancy | Support organization, user, tenant, and permission governance for the server form | Tenant isolation, organization and user system, permission control, token account governance, admin console |

## Documentation

- User/admin/developer manual: `docs/使用说明书/zh-CN/index.md`
- System overview: `docs/wunder系统简介.md`
- Overall design: `docs/总体设计.md`
- API reference: `docs/API文档.md`

## Projects Absorbed by wunder

wunder absorbed code and ideas from quite a few open-source projects along the way:

| Absorbed | Project | URL |
| :--- | :--- | :--- |
| Project Prototype | EVA | https://github.com/ylsdamxssjxxdd/eva |
| Agent Foundation | OpenAI Codex | https://github.com/openai/codex |
| Frontend Foundation | HuLa | https://github.com/HuLaSpark/HuLa |
| Protocol Foundation | Claude Code | https://github.com/anthropics/claude-code |
| User Foundation | OpenClaw | https://github.com/openclaw/openclaw |
