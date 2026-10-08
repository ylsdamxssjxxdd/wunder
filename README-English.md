# wunder Xinjian

- wunder (Xinjian) is an agent orchestration system for organizations and users. It has three runtime forms, each of which can run and be distributed independently:

| Form | Codename | Positioning | Notes |
| :--- | :--- | :--- | :--- |
| server | the Hull | Cloud service | Multi-tenancy, user and organization management, agent app building and publishing, unified gateway access and scheduling; built-in toolchain, knowledge base, and long-term memory; includes an admin frontend (the Bridge) and a user frontend (the Beehive) |
| cli | the Helm | Local command line | Shares the same Engine (wunder-runtime) and Core (wunder-core) with the Hull |
| desktop | the Honeycomb | Local desktop | Shares the same Engine and Core with the Hull; wunder's flagship app, mainly used by individual users |

## Basic Concepts

```text
Xinjian (wunder platform)
├─ User
│  └─ Swarm
│     └─ Agent
│        └─ Thread
└─ ...
```

- User: the top-level principal for isolation and resource ownership
- Swarm: a group of agents organized around one goal
- Agent: the role that does the work — planning, calling models, calling tools
- Thread: the continuous execution context inside an agent

A goal lands on a user, gets divided inside a swarm, and is executed by agents in their own threads.

## Tech Stack

The backend is a single Rust workspace (tokio async runtime) across all forms:

| | server | desktop | cli |
| :--- | :--- | :--- | :--- |
| Backend | Rust + axum 0.8 | Rust, reuses the Engine (wunder-runtime), native in-process calls | Rust, reuses the Engine (wunder-runtime) |
| UI | Vue 3 + TypeScript (built with Vite) | Slint 1.18 native UI, software rendering | Terminal TUI (ratatui + crossterm) |
| Bridge | Plain HTML + JS (web/) | — | — |
| Database | PostgreSQL | SQLite (rusqlite) | SQLite (rusqlite) |
| Access | HTTP / WebSocket | In-process calls, no local networking | Local process |
| Compatibility target | Linux / Docker | Windows 7 x86 and up, plus Linux AppImage | Windows 7 and up (built with the GNU toolchain) |

The Helm (cli) parses arguments with clap; TLS uses rustls (ring provider). The Honeycomb (desktop) uses the native Slint frontend and shares the Engine (wunder-runtime) with the Hull.

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
| Compatibility | Keep the Honeycomb and the Helm explicitly constrained and runnable on legacy operating systems | Windows 7 and Ubuntu 18.04 adaptation, dual Honeycomb/Helm forms, runtime version pinning, dependency downgrade, startup self-check |
| Context Compaction | Control context size in long sessions while retaining useful information | Manual compaction, automatic compaction, overflow recovery, compaction summary re-injection, before/after comparison and replay |
| Sub-agents | Derive subtasks within a single thread, completed by independent executors that return results | Subtask breakdown, sub-agent creation and reclamation, result aggregation, parent-child session linking |
| Scheduled Tasks | Support system-level periodic execution and background governance | Scheduled triggers, planned tasks, background inspection, automatic maintenance, async execution chains |
| Channel Communication | Support multiple entry points into the same runtime capabilities | HTTP, WebSocket, Honeycomb (desktop), Helm (cli), third-party channels, gateway adaptation |
| Memory | Support long sessions and long-term material reuse without polluting the thread's core cognition | Thread-init memory injection, knowledge base, workspace files, long-term material reading |
| Swarm | Support a queen bee dispatching multiple worker bees to complete collaborative tasks | Task breakdown, worker dispatch, node state sync, result aggregation, parent-child session linking |
| Multithreading | Improve throughput and resource utilization through concurrency control within a single process | Worker thread pool, concurrent task scheduling, thread isolation, resource caps, race and deadlock protection |
| Multi-tenancy | Support organization, user, tenant, and permission governance for the Hull (server) | Tenant isolation, organization and user system, permission control, token account governance, Bridge |

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
