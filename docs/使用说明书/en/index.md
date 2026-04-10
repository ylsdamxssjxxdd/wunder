---
title: wunder
summary: wunder is an agent orchestration system that unifies three runtime forms: server, cli, and desktop. Start by choosing the path that matches your role and goal.
read_when:
  - You are learning wunder for the first time
  - You need to decide whether to start with desktop, server, or cli
  - You need an entry point for integration, operations, or tool documentation
source_docs:
  - README.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# wunder

<p class="docs-eyebrow">Agent Orchestration Kernel | server / cli / desktop</p>

## What is wunder?

wunder is an **agent system that executes real tasks**. You give it a goal, and it can break the work down, call tools, coordinate in parallel, and deliver a result.

It is not just a chatbot. It is a **programmable agent execution engine** with three runtime forms:

| Form | Best for | Core value |
|------|----------|------------|
| **Desktop** | Individual users | A local agent workstation that works out of the box |
| **Server** | Teams and organizations | Multi-tenant governance, unified access, and permission control |
| **CLI** | Developers and automation | Terminal-driven workflows, scripting, and pipeline integration |

## What can it do?

Think of it as an AI work team:

- **Files and code**: read files, edit code, run commands, refactor projects
- **Office automation**: organize documents, generate reports, process tables, draft meeting notes
- **Multi-agent collaboration**: one agent researches, one drafts, one reviews, all in parallel
- **Ongoing tasks**: scheduled inspections, recurring reminders, and cross-channel message handling
- **System integration**: connect MCP and external systems, then turn workflows into reusable Skills

## System structure in one minute

![wunder system structure: the layered relationship from wunder to users, swarms, agents, and threads](/docs/assets/manual/08-hierarchy-structure.svg)

Hold on to this main line:

```text
wunder
  └─ User (resource-isolation boundary)
      └─ Swarm (collaboration unit)
          └─ Agent (execution role)
              └─ Thread (continuous context and runtime state carrier)
```

In one sentence: **a request first lands in the user domain, then the swarm orchestrates agents, and execution continues inside threads.**

## Pick the path for your role

<div class="docs-card-grid">
  <a class="docs-card" href="/docs/en/start/desktop/">
    <strong>Individual User</strong>
    <span>Start with desktop and get running in five minutes.</span>
  </a>
  <a class="docs-card" href="/docs/en/start/server/">
    <strong>Team Admin</strong>
    <span>Deploy server and manage users, units, and permissions centrally.</span>
  </a>
  <a class="docs-card" href="/docs/en/start/cli/">
    <strong>Developer</strong>
    <span>Use the CLI for scripting, debugging, and automation.</span>
  </a>
  <a class="docs-card" href="/docs/en/integration/">
    <strong>Integrator</strong>
    <span>Embed wunder into your own system through its public interfaces.</span>
  </a>
  <a class="docs-card" href="/docs/en/ops/">
    <strong>Operator</strong>
    <span>Focus on deployment, monitoring, and performance tuning.</span>
  </a>
  <a class="docs-card" href="/docs/en/tools/">
    <strong>Tool Builder</strong>
    <span>Explore built-in tools, MCP, and Skills.</span>
  </a>
</div>

## Core capabilities at a glance

### Five capability dimensions

| Dimension | What it means |
|------|------|
| **Form convergence** | desktop, server, and cli share the same core engine |
| **Tenant governance** | multi-user, unit trees, token-account governance, and permission control |
| **Agent collaboration** | swarm-based division of labor, parallel execution, and result merging |
| **Tool ecosystem** | built-in tools, MCP, Skills, and knowledge bases |
| **Open interfaces** | WebSocket and SSE streaming, RESTful APIs, and the A2A standard |

### Technical highlights

- **Streaming first**: WebSocket by default, SSE as fallback, with reconnect support
- **Thread freezing**: the system prompt is locked after first initialization to preserve caching
- **Long-term memory**: structured memory fragments with manual and automatic refinement
- **Context compression**: smart summaries plus budget control for long-running conversations
- **Atomic writes**: file writes use temp-file plus rename strategies
- **Tool blowup protection**: layered clipping and budget controls to avoid context explosions

## The easiest mistakes to make

Before going deeper, clear up these common misconceptions:

| Misconception | Correct understanding |
|------|----------|
| The `user_id` for `/wunder` must belong to a registered user | It does not. It can be any virtual identifier |
| Token stats are the same as billing cost | No. Current context occupancy is shown in `round_usage.total_tokens`; total consumption is the sum across requests |
| Every thread rewrites the system prompt every turn | No. It is **frozen** after first initialization |
| Long-term memory is injected every turn | No. It is injected **once at thread initialization** |

## Quick navigation

### First time here?

Start with [Quickstart](/docs/en/start/quickstart/) and get the first working flow running in about ten minutes.

### Want the mental model first?

Start with [Core Concepts](/docs/en/concepts/) and build the system model before diving deeper.

### Need integration guidance?

Go to [Integration Overview](/docs/en/integration/) and choose the access path that fits your system.

### Hit a problem?

See [Troubleshooting](/docs/en/help/troubleshooting/) or [FAQ](/docs/en/help/faq/).

## Further reading

- [Manual Hubs](/docs/en/start/hubs/)
- [API Index](/docs/en/reference/api-index/)
- [System Introduction](/docs/系统介绍.md)
- [Design Plan](/docs/设计方案.md)
