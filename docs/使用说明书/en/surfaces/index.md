---
title: Interface Overview
summary: Multiple forms, multiple interfaces — the Desktop (local AI workbench, the default entry), Android (the Desktop's phone form), the Cloud (cloud AI workbench), the CLI (terminal interface), and the Admin (admin governance console); plus main-area notes and the capability matrix.
read_when:
  - You are choosing between the forms and need to know what each interface can do
  - You expect a capability in one interface and want to check whether it exists there at all
source_docs:
  - crates/wunder-desktop/frontend/ui/theme.slint
  - crates/wunder-desktop/frontend/ui/settings_page.slint
  - frontend/src/router/index.ts
  - web/index.html
  - crates/wunder-cli/tui/app.rs
updated_at: 2026-11-08
---

# Interface Overview

Multiple interfaces map to the forms. They share one engine but differ in how you open them, what you can do, and how they present. The default entry for individual users is the **Desktop** (local AI workbench, a desktop app); **Android** is the Desktop's phone form (local AI workbench, install the APK); the **Cloud** (cloud AI workbench) ships with the Server and is reached in a browser; the **CLI** (terminal interface) serves manual local operation; the **Admin** (admin governance console) serves governance and debugging.

## Interfaces and their users

| Interface | Who uses it | What it is | How to open it |
| --- | --- | --- | --- |
| **Desktop** | Individual users | Local AI workbench | Install the desktop app and open it; it uses no port |
| **Android** | Phone users | The Desktop's phone form, a local AI workbench | Install the APK and open it; data stays on the phone |
| **Cloud** | Team members | Cloud AI workbench | Open the deployed address in a browser |
| **CLI** | Terminal and automation users | Manual local entry point | Start it in a terminal |
| **Admin** | Administrators | Governance and debugging console | Open the Server address in a browser |

## Main areas

The **Desktop**'s main window has **only two pages**: chat and settings. Everything else sits in panels and dialogs — sidebar, input area, timeline, terminal view, tools dialog, companion dialog, workspace dialog, working directory, thread trajectory, subagent details, channels page, scheduled page, settings sections, activity card, expert data, patch-diff card, screenshot overlay, and tray and system notifications. The settings navigation has **11 categories**: General, Account, Models, Tools, Agent, Companion, Scheduled, Memory, Channels, Archive, and Help. There is **no "Device interconnection" category**.

**Android** is a single-window workbench on a phone: the session drawer and model entry at the top, the timeline in the middle, and the input area with its plus panel at the bottom. Settings cover models, appearance, archive, the local runtime, permission management, online updates, and help. Permission management lists notifications, microphone, overlay, accessibility, battery optimization, and install-apps with one-tap jumps to the matching system settings and an automatic refresh on return; online updates live on the Help & About page and switch between the cloud account and GitHub sources, show download progress, and hand a verified APK to the system installer. The working directory, thread trajectory, voice input, and screenshots also have phone entries. The narrow screen uses a single-column layout and reuses the Desktop's Slint components.

The **Cloud** is a single-page app in the browser whose main area is the messenger: left column + timeline + input area, with chat and the working directory joined together. Settings exist as categories, including tools, memory, channels, scheduled tasks, devices, and archive.

The **CLI** runs in a terminal: a chat area above and an input area below, driven by slash commands.

The **Admin** is the governance and debugging console, densely laid out for scanning. Its top-level navigation has **4 groups and 19 items**:

- System (4): Internal status, System settings, Organization management, User management
- Agents (6): Model configuration, Thread management, Preset agents, Companion management, Tool management, System prompts
- Debug (6): Throughput, Performance, Capability evaluation, Multimodal debugging, Debug panel, LSP
- Docs (2): Manual, API

Tool management groups built-in tools, MCP, knowledge bases, skills, and tool visibility together. In the debug panel, the former log area on the right is now a **thread-trajectory view**.

## Capability matrix

The matrix compares the four long-standing interfaces — Desktop, Cloud, Admin, and CLI. Android follows the mobile workbench surface: thread list and timeline, working directory, model and archive settings, voice input, screenshots, and the same tools the agent can call. Details are on each form's own page and in the form matrix.

| Capability | Desktop | Cloud | Admin | CLI |
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

- **Personal daily use**: install the [Desktop](/docs/en/start/desktop/), which covers local files, desktop control, a terminal, screenshots, voice input, and the tray.
- **Phones and mobile use**: install Android, the local AI workbench on a phone — see the [Form matrix](/docs/en/start/forms/).
- **Team collaboration**: once the Server is deployed, members open the [Cloud](/docs/en/surfaces/frontend/) in a browser.
- **Terminal and automation**: use the [CLI](/docs/en/start/cli/); it supports JSON output and cloud subcommands.
- **Admin governance**: use the [Admin](/docs/en/surfaces/web-admin/).

## Boundaries worth clarifying

- **The Admin does not chat**: it is the governance and debugging console, and regular users never see it after signing in.
- **The Cloud and the Admin are not the same interface**: the Cloud faces users and the Admin faces admins, with different routing and visual conventions.
- **The Desktop has no "Device interconnection" settings panel**: the 12 settings categories do not include interconnection; the only interconnection the Desktop offers is in two places — the cloud workspace under "Working directory" in the sidebar, and the approval pop-up for remote commands. The interconnection entry in settings was built but never wired up, so it cannot be opened today.
- **The Desktop cannot play voice (TTS)**: voice input works and TTS models can be configured in settings, but there is no playback. For spoken output, use the Cloud.
- **The CLI cannot create scheduled tasks**: scheduling is explicitly switched off in the CLI, and channels, knowledge bases, long-term memory, and companions have no management interface either.
- **The Admin has no user-level scheduled tasks, channel management, long-term memory, or interconnection panel**: scheduled tasks and channels exist only as templates under "Preset agents"; long-term memory and the interconnection panel were never implemented.
