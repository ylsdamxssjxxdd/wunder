---
title: wunder
summary: wunder is an agent orchestration platform. Install the desktop app (the local AI workbench), describe a goal, and the agent breaks it down, calls tools, and delivers results.
read_when:
  - First time learning about wunder
  - Need to quickly decide where to start
source_docs:
  - README.md
---

# wunder

<p class="docs-eyebrow">An agent orchestration platform</p>

## The Honeycomb: your local AI workbench

You use wunder through the **Honeycomb (desktop)**. It is the local AI workbench — a desktop app covering chat, files, agent settings, and tools. Install it, describe your goal, and the agent breaks the goal down, calls tools, and delivers results — no server deployment required.

```text
Download → Configure a model → Describe your goal → Get results
```

Individual users can start right after installing; [Quick Start](/docs/en/start/quickstart/) gets your first task done in 4 steps.

## What it can do

- **Files & code**: read files, edit code, run commands, refactor projects
- **Office automation**: organize documents, generate reports, process spreadsheets, take meeting notes
- **Task splitting**: when work can run in parallel, the agent spawns temporary subagents, collects their results, and merges them back
- **Continuous tasks**: scheduled checks, recurring reminders, cross-channel message handling
- **System integration**: connect external services, turn recurring flows into skills

## Workbench structure

The Honeycomb uses a two-column layout: the left sidebar holds new task, search, the workspace thread tree, the working directory, and settings; the right side is chat. All daily chat, file handling, agent settings, and tool usage happen here. Every user has exactly one agent instance and one fixed workspace. See [Meet Beehive](/docs/en/surfaces/frontend/).

The system is organized top-down:

![Top-down structure from wunder to thread: wunder, user, agent, and thread progressively land in a concrete conversation.](/docs/assets/manual/08-hierarchy-structure.svg)

You send a message → the agent keeps working in its thread, spawning temporary subagents when work can be split.

## Key features

| Feature | Description |
|------|------|
| **Local-first** | The Honeycomb runs locally, out of the box, and can operate local files and the desktop |
| **One agent per user** | Each user has exactly one agent instance; work that can run in parallel is split across temporary subagents |
| **Rich tool ecosystem** | Built-in tools + MCP external tools + skill packs + knowledge bases |
| **Long sessions** | Context compaction and long-term memory keep long sessions working |
| **Open interfaces** | WebSocket real-time, RESTful API, A2A interop standard |

## Need more? Three optional forms

The Honeycomb covers most scenarios. wunder also ships three optional forms — use them when you need them:

- **The Hull (server)**: the user and agent management platform, deployed only when multiple people need to share one system. Multi-user, permissions, channel access; it provides the Beehive (web, the cloud AI workbench, accessed in a browser) and the Bridge (the admin UI). See the [Hull deployment guide](/docs/en/start/server/).
- **The Beehive (web)**: the cloud AI workbench, provided with the Hull; members access it in a browser. See the [Beehive Interface](/docs/en/surfaces/frontend/).
- **The Helm (cli)**: the manual local entry point for terminals and automation, script-driven. See the [Helm guide](/docs/en/start/cli/).

## Quick navigation

- **First time** → [Quick Start](/docs/en/start/quickstart/)
- **Download & install** → [Honeycomb quick start](/docs/en/start/desktop/)
- **Understand the system** → [Core Concepts](/docs/en/concepts/)
- **Integrate with existing systems** → [Integration Overview](/docs/en/integration/)
- **Running into issues** → [Troubleshooting](/docs/en/help/troubleshooting/) or [FAQ](/docs/en/help/faq/)

## Further reading

- [Documentation Hub](/docs/en/start/hubs/)
- [API Index](/docs/en/reference/api-index/)
