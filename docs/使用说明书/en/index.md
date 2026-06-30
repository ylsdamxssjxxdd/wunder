---
title: wunder
summary: wunder is an agent system that executes tasks. You describe a goal in Hive, and agents break it down, call tools, and deliver results.
read_when:
  - First time learning about wunder
  - Need to quickly decide where to start
source_docs:
  - README.md
  - docs/设计文档/01-系统总体设计.md
---

# wunder

<p class="docs-eyebrow">An agent system that executes tasks</p>

## Hive: your workbench

You use wunder through **Hive**. Hive is the user-side workbench, covering chat, files, agents, tools, and settings. Open Hive, describe your goal, and agents break it down, call tools, and deliver results.

Hive is available in two ways:

| Way | For | Notes |
|------|------|------|
| **Desktop app** | Individual users | Local install, runs out of the box, can operate local files and desktop |
| **Web browser** | Teams / Organizations | Browser access, multi-user, unified management |

Both share the same workbench with identical capabilities. Individual users just install the desktop app; for teams, an admin deploys the server and members access Hive in a browser. Developers and automation scenarios can also use the [CLI](/docs/en/start/cli/).

## What it can do

- **Files & code**: read files, edit code, run commands, refactor projects
- **Office automation**: organize documents, generate reports, process spreadsheets, take meeting notes
- **Multi-agent collaboration**: one agent researches, one drafts, one reviews — in parallel
- **Continuous tasks**: scheduled checks, recurring reminders, cross-channel message handling
- **System integration**: connect external services, turn recurring flows into skills

## Workbench and system structure

Hive uses a three-column layout: left navigation, middle list, right workspace. All daily chat, file management, agent configuration, and tool usage happen here. See [Meet Hive](/docs/en/surfaces/frontend/).

The system is organized top-down:

```
wunder
  └─ User (your space)
      └─ Swarm (collaboration group)
          └─ Agent (the role that works)
              └─ Thread (a continuous conversation)
```

You send a message → the swarm assigns it to the right agent → the agent keeps working in its thread.

## Pick your entry by role

<div class="docs-card-grid">
  <a class="docs-card" href="/docs/en/start/quickstart/">
    <strong>First time</strong>
    <span>Complete your first task.</span>
  </a>
  <a class="docs-card" href="/docs/en/surfaces/frontend/">
    <strong>Meet Hive</strong>
    <span>Chat, files, agents, and tools in Hive.</span>
  </a>
  <a class="docs-card" href="/docs/en/start/desktop/">
    <strong>Individual users</strong>
    <span>Download the desktop app and run locally.</span>
  </a>
  <a class="docs-card" href="/docs/en/start/server/">
    <strong>Team admins</strong>
    <span>Deploy the server, manage users and permissions.</span>
  </a>
  <a class="docs-card" href="/docs/en/surfaces/web-admin/">
    <strong>Admin surface</strong>
    <span>System config, user and channel governance.</span>
  </a>
  <a class="docs-card" href="/docs/en/start/cli/">
    <strong>Developers</strong>
    <span>Terminal-driven, scripting, automation.</span>
  </a>
</div>

## Key features

| Feature | Description |
|------|------|
| **Unified workbench** | Desktop and web share the same Hive, with identical capabilities |
| **Multi-user & permissions** | Layered control over users, organizations, token quotas, and permissions |
| **Agent collaboration** | Multiple agents divide work, execute in parallel, and merge results |
| **Rich tool ecosystem** | Built-in tools + MCP external tools + skill packs + knowledge bases |
| **Open interfaces** | WebSocket real-time, RESTful API, A2A interop standard |

## Quick navigation

- **First time** → [Quick Start](/docs/en/start/quickstart/)
- **Understand the system** → [Core Concepts](/docs/en/concepts/)
- **Integrate with existing systems** → [Integration Overview](/docs/en/integration/)
- **Running into issues** → [Troubleshooting](/docs/en/help/troubleshooting/) or [FAQ](/docs/en/help/faq/)

## Further reading

- [Documentation Hub](/docs/en/start/hubs/)
- [API Index](/docs/en/reference/api-index/)
- [System Overview (design doc)](/docs/设计文档/01-系统总体设计.md)
