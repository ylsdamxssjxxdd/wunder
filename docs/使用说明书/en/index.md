---
title: wunder
summary: wunder is an agent system that executes tasks. You describe a goal in Beehive, and agents break it down, call tools, and deliver results.
read_when:
  - First time learning about wunder
  - Need to quickly decide where to start
source_docs:
  - README.md
  - docs/设计文档/01-系统总体设计.md
---

# wunder

<p class="docs-eyebrow">An agent system that executes tasks</p>

## Beehive: your workbench

You use wunder through **Beehive**. Beehive is the user-side workbench, covering chat, the working directory, agent settings, and tools. Open Beehive, describe your goal, and agents break it down, call tools, and deliver results.

Beehive is available in two ways:

| Way | For | Notes |
|------|------|------|
| **Desktop app** | Individual users | Local install, runs out of the box, can operate local files and desktop |
| **Web browser** | Teams / Organizations | Browser access, multi-user, unified management |

Both share the same workbench with identical capabilities. Individual users just install the desktop app; for teams, an admin deploys the server and members access Beehive in a browser. Developers and automation scenarios can also use the [CLI](/docs/en/start/cli/).

## What it can do

- **Files & code**: read files, edit code, run commands, refactor projects
- **Office automation**: organize documents, generate reports, process spreadsheets, take meeting notes
- **Task splitting**: when work can run in parallel, the agent spawns temporary subagents, collects their results, and merges them back
- **Continuous tasks**: scheduled checks, recurring reminders, cross-channel message handling
- **System integration**: connect external services, turn recurring flows into skills

## Workbench and system structure

Beehive uses a two-column layout: the left sidebar holds new task, search, the workspace thread tree, the working directory, and settings; the right side is chat. All daily chat, file handling, agent settings, and tool usage happen here. Every user has exactly one agent instance and one fixed cloud working directory. See [Meet Beehive](/docs/en/surfaces/frontend/).

The system is organized top-down:

![Top-down structure from wunder to thread: wunder, user, agent, and thread progressively land in a concrete conversation.](/docs/assets/manual/08-hierarchy-structure.svg)

You send a message → the agent keeps working in its thread, spawning temporary subagents when work can be split.

## Pick your entry by role

<div class="docs-card-grid">
  <a class="docs-card" href="/docs/en/start/quickstart/">
    <strong>First time</strong>
    <span>Complete your first task.</span>
  </a>
  <a class="docs-card" href="/docs/en/surfaces/frontend/">
    <strong>Meet Beehive</strong>
    <span>Chat, the working directory, agent settings, and tools.</span>
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
| **Unified workbench** | Desktop and web share the same Beehive, with identical capabilities |
| **Multi-user & permissions** | Layered control over users, organizations, token quotas, and permissions |
| **One agent per user** | Each user has exactly one agent instance, bound 1:1 to an admin-side preset; work that can run in parallel is split across temporary subagents |
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
