# wunder Xinjian

> One-liner: Xinjian is an agent system that executes tasks, not just chats. You provide the goal; it plans, calls tools, and delivers results.

The primary entry is **wunder-desktop** (download and use), while the same core also powers `wunder-server` and `wunder-cli`.

## What This README Solves

- You are new to wunder and want a practical overview first
- You want to run it in 3 steps before reading deep architecture docs
- You want to understand the hierarchy: `Xinjian -> User -> Swarm -> Agent -> Thread`

## System Hierarchy Diagram

```text
Xinjian (wunder platform)
├─ User 1
│  ├─ Swarm 1
│  │  ├─ Agent 1
│  │  │  ├─ Thread 1
│  │  │  └─ Thread 2
│  │  └─ Agent 2
│  └─ Swarm 2
│     └─ ...
├─ User 2
│  └─ ...
└─ ...
```

- `User`: top-level isolation boundary for sessions, workspace, and ownership.
- `Swarm`: a collaboration unit that groups agents around one goal.
- `Agent`: an execution role that plans, calls models/tools, and produces output.
- `Thread`: the continuous execution context inside an agent.

In short: **requests enter a user scope, are split in swarms, and are executed by agents through threads.**

## Start in 3 Steps

1. Open Releases and download the `wunder-desktop` package for your OS (release page is source of truth).
2. Install/unzip and launch `wunder-desktop`.
3. Type your task in the input box and press Enter.

Tip: if you only want to try it once, this is enough.

## Choose by Goal

| Your Goal | Recommended Entry | Why |
| --- | --- | --- |
| I just want to use it now | `wunder-desktop` | Lowest setup cost, local-first experience |
| I need team collaboration and governance | `wunder-server` | Multi-user, multi-tenant, unified access and control |
| I need scripted automation | `wunder-cli` | Terminal workflows and pipeline-friendly execution |

## What You Can Ask Xinjian to Do

- Office work: docs, proposals, summaries, meeting notes, tables, slides
- Engineering work: coding, refactoring, script generation, troubleshooting
- Integration work: combine MCP + Skills to build repeatable workflows
- Ongoing automation: scheduled tasks, periodic checks, channel-based handling
- Bootstrap: use wunder to build wunder (code, docs, and operations)

## Capability Snapshot (Current Real Features)

| Capability | Description | Common Uses |
| --- | --- | --- |
| Desktop control and automation | Local desktop can operate apps, files, and web after authorization | Organize files, batch processing, generate documents |
| MCP tool ecosystem | Supports `/wunder/mcp`, self-hosted and cross-system tool access | Connect external services, call tools across systems |
| Skills workflows | Skills package repeatable steps and workflows | Reports, one-click standard processes |
| Multi-agent parallel collaboration | Multiple agents work in parallel and hand off tasks | One researches, one writes, one reviews |
| Scheduling and orchestration | Gateway/channel/scheduler for long-running automation | Recurring cleanups, reminders, periodic jobs |
| Long-session continuity | Context compaction plus long-term memory | Ongoing projects, iterative refinement |
| Multi-channel | Every agent can connect to QQ, Feishu, and other messaging platforms | Consistent cross-client experience, stable access |

## Three Runtime Forms

| Form | Best for | One-liner |
| --- | --- | --- |
| **wunder-desktop (Recommended)** | Individuals / general users | Download-and-use local desktop agent |
| `wunder-server` | Teams / organizations | Unified access, permission management, multi-tenant collaboration |
| `wunder-cli` | Developers / automation | Command-line driven and script-oriented execution |

> Fastest path: desktop. Team governance and integration: server. Script automation: cli.

## Common Misunderstandings

- `user_id` in `/wunder` does not have to be a registered account.
- Token stats represent **context occupancy**, not total billed consumption.
- Sessions are split into user rounds and model rounds.
- WebSocket is preferred; SSE is fallback.

## FAQ

**Q: Do I need to deploy server first?**  
A: No. Most users can start directly with `wunder-desktop`. Use server when team-level governance and deep integrations are required.

**Q: Do I need technical skills?**  
A: No. Describe the objective and the agent handles the execution path.

**Q: How do I trigger multi-agent mode?**  
A: Just provide the objective. The system can split work automatically, or you can explicitly ask for parallel execution.

## Documentation Entry Points

- Static docs homepage: `docs/静态站文档/zh-CN/index.md`
- System overview: `docs/系统介绍.md`
- Design doc: `docs/设计方案.md`
- API doc: `docs/API文档.md`

## Projects Absorbed by wunder

| Absorbed | Project Name | GitHub URL |
| :--- | :--- | :--- |
| Agent Foundation | EVA | https://github.com/ylsdamxssjxxdd/eva |
| Rust Foundation | OpenAI Codex | https://github.com/openai/codex |
| Frontend Foundation | HuLa | https://github.com/HuLaSpark/HuLa |
| MCP/SKILLS | Claude Code | https://github.com/anthropics/claude-code |
| Gateway/Channel/Scheduled Tasks | OpenClaw | https://github.com/openclaw/openclaw |
| Agent LSP | OpenCode | https://github.com/anomalyco/opencode |
| Swarm Canvas | clawport-ui | https://github.com/JohnRiceML/clawport-ui |
