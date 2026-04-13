---
title: Desktop Interface
summary: `wunder-desktop` is the current primary delivery form, emphasizing local-first and desktop workbench.
read_when:
  - You want to use Wunder directly
  - You want to understand why desktop is the current product focus
source_docs:
  - README.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# Desktop Interface

If you think of Wunder as a product rather than a backend project, the most important interface right now is the desktop client.

## What It Is

`wunder-desktop` is not simply a web page wrapped in a native shell.

Its goal is to combine these elements into a "local-first agent workbench":

- User-side messaging workbench
- Local bridge service
- Local working directory
- Local run mode
- Desktop capability access

## Why Desktop Is the Current Focus

Because it validates Wunder's core value chain in a single package:

- Conversation entry point
- Tool execution
- Workspace artifacts
- Local file capabilities
- Visualized agent loop

## Current Characteristics

- Local mode is the default
- Uses the bundled Python runtime by default
- Supports a persistent local working directory
- Reuses the unified `/wunder` orchestration kernel
- No longer provides a server connection switch within the desktop client

## Relationship with the User Frontend

Desktop reuses the same page structure as the user-side frontend wherever possible.

This means:

- Most page interaction logic comes from `frontend/`
- What the desktop client adds are local bridging, system settings, directory mapping, and runtime-mode capabilities

## Who It Suits Best

- Regular individual users
- People who need local files and a desktop environment
- People who want to get started without deploying a full server first

## What If You Need Server Capabilities

The desktop client now only maintains local mode.

If you need the following capabilities, use a browser to access the server directly instead of switching connection modes inside the desktop client:

- Multi-user and multi-tenant
- Unified admin governance
- Organization-level deployment
- Unified channel and gateway access

## Further Reading

- [Getting Started with Desktop](/docs/en/start/desktop/)
- [User Frontend](/docs/en/surfaces/frontend/)
- [Deployment and Operations](/docs/en/ops/deployment/)