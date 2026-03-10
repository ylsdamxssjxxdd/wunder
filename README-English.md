# wunder

One-liner: wunder is a desktop agent that can **fully take over your computer (with your permission)** and do office work for you. We now focus on **wunder-desktop** - download, install, and use.

It is not just chat. You state the goal, it breaks down the work, calls tools, and delivers results. When needed, it also supports **multi-agent parallel collaboration** so several agents can work on different parts at the same time.

## Capability Snapshot (based on real wunder features)
| Capability | Description | Common Uses |
| --- | --- | --- |
| Desktop control and automation | Local desktop can operate apps, files, and web after authorization | Organize files, batch processing, generate documents |
| MCP tool ecosystem | Supports `/wunder/mcp`, self-hosted and cross-system tool access | Connect external services, call tools across systems |
| Skills workflows | Skills package repeatable steps and workflows | Reports, one-click standard processes |
| Multi-agent parallel collaboration | Multiple agents work in parallel and hand off tasks | One researches, one writes, one reviews |
| Scheduling and orchestration | Gateway/channel/scheduler capabilities for long-running automation | Recurring cleanups, reminders, periodic jobs |
| Long-session continuity | Context compaction plus long-term memory | Ongoing projects, iterative refinement |
| Unified access and channels | Server provides unified entry, WS first + SSE fallback | Consistent multi-endpoint access |

## Hive Protocol (HPP) and Multi-Agent Asset Packaging
- `Hive`: a collaboration unit that groups multiple agent roles around one business objective.
- `WorkerPack`: role-level package describing responsibilities, constraints, and attached skills.
- `HivePack`: distributable package containing hive manifest + multiple worker packs + skill resource declarations.
- After import, the system auto-creates and assigns agents, auto-enables skills, and mounts built-in/MCP/knowledge tools by default for fast adaptation.
- Conflict handling supports two policies:
  - `auto_rename_only` (default): rename conflicting items and preserve existing local assets.
  - `update_replace`: replace same-name items with package content for upgrade-style rollout.

## What You Can Do
- Write docs, proposals, summaries, tables, and slides
- Search and synthesize information, extract key points
- Write code, refactor, generate scripts and automation
- Combine MCP + Skills to build your own workflows
- **Build wunder itself (bootstrap)** - let wunder write code, docs, and run tasks

## Quick Start (3 steps)
1. Open the Releases page and download the wunder-desktop package for your system.
2. Install/unzip and launch wunder-desktop.
3. Describe your task in the input box and press Enter.

Tip: If you only want to try it, this is all you need.

## Three Forms, Choose What You Need
| Form | Best for | One-liner |
| --- | --- | --- |
| **wunder-desktop (Recommended)** | General users / individuals | Download-and-use desktop agent |
| `wunder-server` | Teams / organizations | Unified access, permissions, multi-tenant collaboration |
| `wunder-cli` | Developers / automation | Command-line driven tasks and scripts |

> Want the fastest start? Use **wunder-desktop**. Need team collaboration and system integrations? Look into server. Need scripting? Choose cli.

## FAQ
**Q: Do I need to deploy server first?**  
A: No. wunder-desktop is the recommended entry. You can use it immediately. Consider server only if you need team collaboration or deeper integrations.

**Q: Do I need technical skills?**  
A: Not at all. It is designed for everyday users - just state your goal.

**Q: How do I use multi-agent mode?**  
A: Simply ask for parallel work. The system can split tasks automatically, or you can request specific parallel roles.

## Developer Docs (optional)
- `docs/系统介绍.md`
- `docs/设计方案.md`
- `docs/API文档.md`

## wunder Devoured Core
| Devoured | Project Name | GitHub URL |
| :--- | :--- | :--- |
| Agent Foundation | EVA | https://github.com/ylsdamxssjxxdd/eva |
| Rust Foundation | OpenAI Codex | https://github.com/openai/codex |
| Frontend Foundation | HuLa | https://github.com/HuLaSpark/HuLa |
| MCP/SKILLS | Claude Code | https://github.com/anthropics/claude-code |
| Gateway/Channel/Cron Jobs | OpenClaw | https://github.com/openclaw/openclaw |
| Agent LSP | OpenCode | https://github.com/anomalyco/opencode |
