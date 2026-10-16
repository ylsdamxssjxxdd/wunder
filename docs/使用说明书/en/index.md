---
title: wunder
summary: The four forms of wunder, its capability range, parallel tasks and the session queue, plus navigation entries into the manual.
read_when:
  - You are new to wunder and choosing which form to use
  - You want the entry point to the whole manual
source_docs:
  - frontend-slint/ui/theme.slint
  - frontend/src/router/index.ts
  - web/index.html
  - crates/wunder-server/src/main.rs
updated_at: 2026-10-16
---

# wunder

<p class="docs-eyebrow">An agent orchestration platform</p>

Give it a goal, and the agent breaks the task down on its own, calls tools, and hands the results back.

## Forms

The four forms are built on the same engine:

| Form | Positioning | Who it's for |
| --- | --- | --- |
| **Pupa** | Local AI workbench, installed on your own computer | Individual users (the default entry point) |
| **Beehive** | Cloud AI workbench, opened in a browser | Team members after the Hull is deployed |
| **Helm** | Manual local entry point, used in a terminal | Command-line users / automation scripts |
| **Hull** | User and agent management platform, shared by a team | Administrators |

See the [form matrix](/docs/en/start/forms/) for the full comparison of tech stack, run model and OS support.

The Pupa and the Helm don't depend on the Hull and can be installed and run on their own. The **Bridge** is the management interface that ships with the Hull, starts alongside it, and is aimed at administrators.

Deploy the rest as needed: the **Hull (server)** covers shared use, permission management, channel access, and unified publishing — see [Hull deployment](/docs/en/start/server/); the **Beehive (web)** ships with the Hull, and members open it in a browser once it is deployed; the **Helm (cli)** covers terminal and automation scenarios — see [Using the Helm](/docs/en/start/cli/).

## Default Entry Point

Individual users start with the Pupa: download and install, and it opens as a local AI workbench.

```text
Download the Pupa → Configure a model → Describe your goal → Get results
```

See [Quick Start](/docs/en/start/quickstart/) and [Pupa quick start](/docs/en/start/desktop/) for the details.

## Capability Range

- **Files and code**: read, edit, run commands, refactor projects
- **Office automation**: organize documents, generate reports, process spreadsheets, take meeting notes
- **Parallel subtasks**: when several lines of work need to move at once, the main agent spins up a few subagents to work in parallel, then merges their results as they come back
- **Scheduled tasks**: periodic checks, recurring reminders, cross-channel message handling
- **External service connections**: turn common flows into skills
- **Device interconnection**: once a local device joins the cloud, the cloud can issue commands to the local node with your approval

## Concurrent Tasks and the Session Queue

- One agent can carry **several independent task threads at the same time**, without interfering with each other.
- You can also have several workspaces: a local directory, a cloud workspace, and a remote device workspace reached through device interconnection, distinguished by name in the interface.
- Sending a message mid-run won't bounce back: the **session queue** is on by default, so new messages wait in the queue instead of hitting a "user busy" error.

## Interfaces

- [Pupa interface](/docs/en/surfaces/desktop-ui/): the main window has two pages, "Chat" and "Settings"; everything else appears as panels and dialogs
- [Beehive interface](/docs/en/surfaces/frontend/): a single-page app in the browser
- [Bridge interface](/docs/en/surfaces/web-admin/): the administrator's governance and debugging backend
- [Interface Overview](/docs/en/surfaces/index/): how the four forms compare

## Navigation

- **First time** → [Quick Start](/docs/en/start/quickstart/)
- **Download and install** → [Pupa quick start](/docs/en/start/desktop/)
- **Interface map** → [Interface Overview](/docs/en/surfaces/index/)
- **Understand the system in depth** → [Core Concepts](/docs/en/concepts/)
- **Documentation hub** → [Documentation Hub](/docs/en/start/hubs/)
- **Doc ecosystem navigation** → [Common Docs Directory](/docs/en/start/docs-directory/)
- **First launch fails or something goes wrong** → [Troubleshooting](/docs/en/help/troubleshooting/) or [FAQ](/docs/en/help/faq/)
