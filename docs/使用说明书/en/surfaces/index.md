---
title: Interface Overview
summary: Four forms, four interfaces — the Pupa (local AI workbench, the default entry), the Beehive (cloud AI workbench), the Helm (terminal interface), and the Bridge (admin governance console); plus main-area notes and the capability matrix.
read_when:
  - You are choosing between the forms and need to know what each interface can do
  - You expect a capability in one interface and want to check whether it exists there at all
source_docs:
  - frontend-slint/ui/theme.slint
  - frontend-slint/ui/settings_page.slint
  - frontend/src/router/index.ts
  - web/index.html
  - crates/wunder-cli/tui/app.rs
updated_at: 2026-10-16
---

# Interface Overview

Four interfaces map to the four forms. They share one engine but differ in how you open them, what you can do, and how they present. The default entry for individual users is the **Pupa** (local AI workbench, a desktop app); the **Beehive** (cloud AI workbench) ships with the Hull and is reached in a browser; the **Helm** (terminal interface) serves manual local operation; the **Bridge** (admin governance console) serves governance and debugging.

## Interfaces and their users

| Interface | Who uses it | What it is | How to open it |
| --- | --- | --- | --- |
| **Pupa** | Individual users | Local AI workbench | Install the desktop app and open it; it uses no port |
| **Beehive** | Team members | Cloud AI workbench | Open the deployed address in a browser |
| **Helm** | Terminal and automation users | Manual local entry point | Start it in a terminal |
| **Bridge** | Administrators | Governance and debugging console | Open the Hull address in a browser |

## Main areas

The **Pupa**'s main window has **only two pages**: chat and settings. Everything else sits in panels and dialogs — sidebar, input area, timeline, terminal view, tools dialog, companion dialog, workspace dialog, working directory, thread trajectory, subagent details, channels page, scheduled page, settings sections, activity card, expert data, patch-diff card, screenshot overlay, and tray and system notifications. The settings navigation has **12 categories**: General, Account, Models, Tools, Agent, Companion, Scheduled, Memory, Channels, Prompt packs, Archive, and Help. There is **no "Device interconnection" category**.

The **Beehive** is a single-page app in the browser whose main area is the messenger: left column + timeline + input area, with chat and the working directory joined together. Settings exist as categories, including tools, memory, channels, scheduled tasks, devices, and archive.

The **Helm** runs in a terminal: a chat area above and an input area below, driven by slash commands.

The **Bridge** is the governance and debugging console, densely laid out for scanning. Its top-level navigation has **4 groups and 19 items**:

- System (4): Internal status, System settings, Organization management, User management
- Agents (6): Model configuration, Thread management, Preset agents, Companion management, Tool management, System prompts
- Debug (6): Throughput, Performance, Capability evaluation, Multimodal debugging, Debug panel, LSP
- Docs (2): Manual, API

Tool management groups built-in tools, MCP, knowledge bases, skills, and tool visibility together. In the debug panel, the former log area on the right is now a **thread-trajectory view**.

## Capability matrix

What each interface has and lacks; details are on each form's own page.

| Capability | Pupa | Beehive | Bridge | Helm |
| --- | --- | --- | --- | --- |
| Thread list + timeline | ✅ | ✅ | ✅ (thread management) | ✅ |
| Archive | ✅ category in settings | ✅ | Unverified | ✅ restored by command |
| Tools / MCP / skills panel | ✅ tools dialog | ✅ settings Tools category (built-in / MCP / skills / knowledge bases) | ✅ tool management group | ✅ matching subcommands |
| Knowledge bases | ✅ | ✅ | ✅ | ❌ not implemented |
| Long-term memory | ✅ settings + expert-data card | ✅ | ❌ not implemented | ❌ not implemented |
| Scheduled tasks | ✅ | ✅ | ⚠️ only as a "Preset agents" template | ❌ switched off |
| Channel binding | ✅ | ✅ | ⚠️ only as a "Preset agents" template | ❌ not implemented |
| Companion system | ✅ including the desktop floating window | ✅ | ✅ | ❌ not implemented |
| Workspace files | ✅ | ✅ | ✅ | ⚠️ command-line file operations and file references only |
| Interactive terminal | ✅ | ❌ not implemented | ❌ not implemented | Not applicable (it already runs in a terminal) |
| Device interconnection | ⚠️ sidebar cloud workspace + approval pop-up; settings category not opened | ✅ settings Devices category + interconnection view | ❌ not implemented (backend API only) | ✅ matching subcommands |
| Voice input / TTS | Input ✅ / output ❌ | Both directions ✅ | ❌ | No speech recognition (paste / drag in images only) |
| Screenshot capture | ✅ | ❌ | ❌ | ❌ |
| Tray / system notifications / global hotkey | ✅ | ❌ | ❌ | ❌ |

## Choosing by scenario

- **Personal daily use**: install the [Pupa](/docs/en/start/desktop/), which covers local files, desktop control, a terminal, screenshots, voice input, and the tray.
- **Team collaboration**: once the Hull is deployed, members open the [Beehive](/docs/en/surfaces/frontend/) in a browser.
- **Terminal and automation**: use the [Helm](/docs/en/start/cli/); it supports JSON output and cloud subcommands.
- **Admin governance**: use the [Bridge](/docs/en/surfaces/web-admin/).

## Boundaries worth clarifying

- **The Bridge does not chat**: it is the governance and debugging console, and regular users never see it after signing in.
- **The Beehive and the Bridge are not the same interface**: the Beehive faces users and the Bridge faces admins, with different routing and visual conventions.
- **The Pupa has no "Device interconnection" settings panel**: the 12 settings categories do not include interconnection; the only interconnection the Pupa offers is in two places — the cloud workspace under "Working directory" in the sidebar, and the approval pop-up for remote commands. The interconnection entry in settings was built but never wired up, so it cannot be opened today.
- **The Pupa cannot play voice (TTS)**: voice input works and TTS models can be configured in settings, but there is no playback. For spoken output, use the Beehive.
- **The Helm cannot create scheduled tasks**: scheduling is explicitly switched off in the Helm, and channels, knowledge bases, long-term memory, and companions have no management interface either.
- **The Bridge has no user-level scheduled tasks, channel management, long-term memory, or interconnection panel**: scheduled tasks and channels exist only as templates under "Preset agents"; long-term memory and the interconnection panel were never implemented.
