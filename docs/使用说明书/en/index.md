---
title: wunder
summary: wunder is an agent orchestration platform. The Honeycomb is the local AI workbench on your computer, the Beehive is the cloud workbench in a browser, the Helm is the terminal entry point, and the Hull is the shared management platform for teams.
read_when:
  - You are new to wunder and choosing which form to use
  - You want the entry point to the whole manual
source_docs:
  - frontend-slint/ui/theme.slint
  - frontend/src/router/index.ts
  - web/index.html
  - crates/wunder-server/src/main.rs
updated_at: 2026-10-13
---

# wunder

<p class="docs-eyebrow">An agent orchestration platform</p>

wunder is an agent orchestration platform. You give it a goal, and it breaks the task down on its own, calls tools, and hands the results back.

## Four Forms by Scenario

One engine, four shells:

| Form | In one line | Who it's for |
| --- | --- | --- |
| **Honeycomb** | An AI workbench installed on your own computer | Individual users (the default entry point) |
| **Beehive** | A cloud workbench you open in a browser | Team members after the Hull is deployed |
| **Helm** | Command the agent from the terminal | Command-line users / automation scripts |
| **Hull** | A shared management and governance platform | Administrators |

The Honeycomb and the Helm don't depend on the Hull and can be installed and run on their own. The **Bridge** is the management interface that ships with the Hull, starts alongside it, and is aimed at administrators.

## Start with the Honeycomb

Individual users go with the Honeycomb by default: download and install, and it opens as a local AI workbench.

```text
Download the Honeycomb → Configure a model → Describe your goal → Get results
```

See [Quick Start](/docs/en/start/quickstart/) and [Honeycomb quick start](/docs/en/start/desktop/) for the details.

## What It Can Do

- **Files and code**: read, edit, run commands, refactor projects
- **Office automation**: organize documents, generate reports, process spreadsheets, take meeting notes
- **Parallel subtasks**: when several lines of work need to move at once, the main agent spins up a few subagents to work in parallel, then merges their results as they come back
- **Scheduled tasks**: periodic checks, recurring reminders, cross-channel message handling
- **External service connections**: turn common flows into skills
- **Device interconnection**: once a local device joins the cloud, the cloud can issue commands to the local node with your approval

## One Agent, Many Parallel Tasks

- One agent can carry **several independent task threads at the same time**, without interfering with each other.
- You can also have several workspaces: a local directory, a cloud workspace, and a remote device workspace reached through device interconnection, distinguished by name in the interface.
- Sending a message mid-run won't bounce back: the **session queue** is on by default, so new messages wait in the queue instead of hitting a "user busy" error.

## Interface at a Glance

- [Honeycomb interface](/docs/en/surfaces/desktop-ui/): the main window has two pages, "Chat" and "Settings"; everything else appears as panels and dialogs
- [Beehive interface](/docs/en/surfaces/frontend/): a single-page app in the browser
- [Bridge interface](/docs/en/surfaces/web-admin/): the administrator's governance and debugging backend
- [Interface Overview](/docs/en/surfaces/index/): how the four forms compare

## Further Reading

The Honeycomb covers most scenarios; add more as needed:

- **Hull (server)**: shared use, permission management, channel access, unified publishing. See [Hull deployment](/docs/en/start/server/).
- **Beehive (web)**: after the Hull is deployed, members open the cloud workbench in a browser.
- **Helm (cli)**: terminal and automation scenarios. See [Using the Helm](/docs/en/start/cli/).

## Quick Navigation

- **First time** → [Quick Start](/docs/en/start/quickstart/)
- **Download and install** → [Honeycomb quick start](/docs/en/start/desktop/)
- **Interface map** → [Interface Overview](/docs/en/surfaces/index/)
- **Understand the system in depth** → [Core Concepts](/docs/en/concepts/)
- **Integrate with existing systems** → [Integration Overview](/docs/en/integration/)
- **Documentation hub** → [Documentation Hub](/docs/en/start/hubs/)
- **Doc ecosystem navigation** → [Common Docs Directory](/docs/en/start/docs-directory/)
- **First launch fails or something goes wrong** → [Troubleshooting](/docs/en/help/troubleshooting/) or [FAQ](/docs/en/help/faq/)
