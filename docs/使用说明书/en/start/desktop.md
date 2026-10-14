---
title: Starting with the Pupa (Desktop)
summary: wunder's default way to work: a local AI workbench. Download, install, open, go — no Hull to deploy first; it calls the engine natively in-process, takes no port, and routes through no HTTP/WebSocket relay.
read_when:
  - You are installing or first launching the Pupa
  - You need to know where its local data lives or how to migrate it
source_docs:
  - frontend-slint/Cargo.toml
  - frontend-slint/run.bat
  - crates/wunder-desktop/args.rs
  - crates/wunder-desktop/native.rs
  - crates/wunder-desktop/runtime.rs
  - crates/wunder-runtime/src/services/desktop_runtime_recovery.rs
updated_at: 2026-10-13
---

# Starting with the Pupa (Desktop)

The Pupa (desktop) is wunder's desktop edition and the **default way to work**: a **local AI workbench**. It needs no Hull (server) deployment. It calls the engine natively in-process, has no port of its own, and routes through no HTTP/WebSocket relay.

## When to choose the Pupa

| Scenario | Choose the Pupa |
| --- | --- |
| Want to start right away, without deploying a service first | ✅ |
| Need to work with local files, windows, and browsers | ✅ |
| Prefer a graphical interface | ✅ |
| Need an interactive terminal and screenshot capture | ✅ |
| Need a tray, system notifications, and global hotkeys | ✅ |
| May connect to a remote later, but want to get running now | ✅ |
| Need multi-user governance and an admin backend | ❌ Choose the Hull (server) |
| Need to access it in a browser | ❌ Choose the Beehive (web, ships with the Hull) |

## Install and launch

- **Executable**: `wunder-frontend-slint`
- **Development launch**: on Windows, use the project's bundled `run.bat`
- **Platforms**: primarily targets 32-bit Win7 and Ubuntu 18.04; modern Windows / Linux also run
- **No port, no required arguments**: it calls the engine in-process right after launch
- There's also a compatibility bridge port (default 18123) that serves only legacy-compatible clients; day-to-day use doesn't go through it
- The Electron / Tauri frontend shells are **no longer maintained day-to-day and get no new features** (this doesn't mean the Pupa backend stops being maintained)

## Local data

| Content | Location |
| --- | --- |
| Root directory | `WUNDER_HOME` → `~/.wunder` by default |
| Workspace | `~/.wunder/workspace` |
| Temporary directory | `WUNDER_DESKTOP_TEMP_ROOT` |
| Desktop settings | `<temp>/config/desktop.settings.json` |
| Sessions and tool logs | local SQLite |

To move to a new machine, copy `~/.wunder` across. On startup it scans the work tree (up to 4096 items), restores the state of unfinished tasks from last time, and clears session locks and leftover tasks.

## Interface layout (the main window has only two pages)

The main window has only two pages, Chat and Settings; everything else hangs off them as panels and dialogs.

### Chat page

- **Left column**: the task and thread tree, the **working-directory area** (local file tree + usage bar + **cloud workspaces**), and the settings entry at the bottom
- **Right column**: the top bar (workspace / thread name) + timeline + composer
- With no thread selected, a welcome page shows; file preview and online editing pop out as overlays without changing the two-column structure
- One agent can run several **independent task threads** in parallel, and workspaces can be **multiple**: local + cloud + remote device workspaces reached through device interlink

### Settings page

**12 categories**: General, Account, Model, Tools, Agents, Personas, Scheduled, Memory, Channels, Prompt packs, Archive, Help.

The category navigation supports search filtering. There is **no "device interlink" category**.

## Get running in 5 steps

### 1. Download and install

Download the installer for your system from Releases.

### 2. First launch

On launch, a lightweight splash screen appears first while local working directories, configuration, and sessions are prepared. The window appearing doesn't mean sessions are ready; you can start working once the workbench opens. First launches or slower devices may take longer to prepare.

### 3. Configure the model

Settings → Model → Add:

- API Key
- Endpoint URL (a neutral endpoint such as `https://example.com/v1`)
- Model name and parameters (temperature, context length, and so on)

Confirm with "Test connection" before saving. This is also where you hook up TTS, image, and video models; **TTS can currently only be configured — there is no playback implementation**.

### 4. Complete your account profile

Settings → Account: username, email, avatar; set a login password if you plan to use it long-term.

### 5. Start your first conversation

Back on the chat page, type:

```
List the files in the current working directory
```

When you see the model think → a batch of tools → the final reply landing in the chat area, you're up and running.

## What only the Pupa can do

- **Interactive terminal** (winpty on Windows, PTY on Linux)
- **Screenshot capture** (X11 on Linux, Win32 APIs on Windows)
- **Tray and system notifications**
- **Global hotkeys**
- **Voice input**

## About device interlink

Only **two** interlink surfaces are available in the Pupa:

1. The **cloud workspaces** in the sidebar's working-directory area: when the network is unavailable they clearly show "offline" alongside a "connect" button
2. The **remote command approval dialog**: high-risk commands (L3) in device interlink pop up for confirmation with a red border, showing the command level, risk, and source, plus a "don't remember" hint

The Settings page still holds a set of device-interlink options, but they aren't wired to any interface; **that entry point is not open yet**.

## One-click workspace state reset

When sessions, subagents, or workspaces get into a bad state, use "Settings → General → Reset workspace state":

- **It does**: stop running sessions, clear queued tasks, clean up session locks
- **It keeps**: skills, knowledge bases, long-term memory, user configuration, workspace files, session history

It **clears working state**; it is not a factory reset.

## Two interaction constraints

- **The session queue is on by default**: sending another message during a run queues it instead of rejecting it as "user busy." Turn it off and messages sent during a run are bounced outright without entering the queue bar.
- **The subagent activity indicator**: when a task has spawned subagents, the chat area shows the child runs that are still working; subagents are temporary work units the main agent creates, reclaimed once the task ends.

## Common questions

**Where data lives**
The workspace is under `~/.wunder/workspace`, session history is in local SQLite, and configuration is in `<temp>/config/desktop.settings.json`.

**Migrating to another computer**
Yes. Copy `~/.wunder` to the new machine.

**Networking in local mode**
Yes. "Local first" only means data stays on your machine; it doesn't affect reaching remote models or web pages.

**Sharing among several people**
The Pupa focuses on single-user local scenarios; for team collaboration, deploy the Hull and have members use the Beehive.

## Further reading

- [Quick Start](/docs/en/start/quickstart/)
- [Pupa Interface](/docs/en/surfaces/desktop-ui/)
- [Surfaces Overview](/docs/en/surfaces/index/)
- [First-Run Troubleshooting](/docs/en/start/troubleshooting-first-run/)
- [Troubleshooting](/docs/en/help/troubleshooting/)
