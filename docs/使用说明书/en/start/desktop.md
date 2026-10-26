---
title: Starting with the Pupa (Desktop)
summary: The local AI workbench: install and launch, layout of local data, the two-page main window, first-run configuration, capabilities unique to the desktop, and interaction constraints.
read_when:
  - You are installing or first launching the Pupa
  - You need to know where its local data lives or how to migrate it
source_docs:
  - frontend-slint/Cargo.toml
  - frontend-slint/run.bat
  - crates/wunder-desktop/args.rs
  - crates/wunder-desktop/native.rs
  - crates/wunder-desktop/runtime.rs
updated_at: 2026-10-16
---

# Starting with the Pupa (Desktop)

It calls the engine natively in-process, has no port of its own, and routes through no HTTP/WebSocket relay. Individual users install just this — there is no Hull (server) to deploy first.

## Prerequisites

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

The Pupa covers single-user local scenarios; sharing among several people runs through the Hull and the Beehive.

## Install and launch

Download and run the installer for your system. It installs the Pupa and Helm (CLI) by default; optional Python/Git/rg/ffmpeg/browser components are opt-in. The install directory defaults to `~/.wunder` and can be changed to any writable directory. The installer writes `.wunder-home` at the install root, and both local forms use that root as `WUNDER_HOME`; an explicit non-empty `WUNDER_HOME` still wins.

- **Executable**: `bin/wunder-desktop.exe` (Windows) or `bin/wunder-desktop` (Linux) after installation; development builds use `wunder-frontend-slint`
- **Terminal commands**: the installer adds the install root's `bin` to the user PATH (Windows) or creates command links under `~/.local/bin` (Linux); after opening a new terminal, `wunder-cli` (Helm) and `desktop` / `wunder-desktop` (Pupa) run directly
- **Menu and desktop entry**: Windows names the Start Menu and optional desktop shortcuts Pupa and Helm; Linux registers a Pupa application-menu entry and creates a Pupa launcher in the XDG desktop directory by default
- **Development launch**: on Windows, use the project's bundled `run.bat`
- **Platforms**: primarily targets 32-bit Win7 and Ubuntu 18.04; modern Windows / Linux also run
- **No port, no required arguments**: it calls the engine in-process right after launch
- There's also a compatibility bridge port (default 18123) that serves only legacy-compatible clients; day-to-day use doesn't go through it
- The Electron / Tauri frontend shells are **no longer maintained day-to-day and get no new features** (this doesn't mean the Pupa backend stops being maintained)

## Local data

| Content | Location |
| --- | --- |
| Root directory | The directory chosen at install time (`WUNDER_HOME`; defaults to `~/.wunder`, recorded by `.wunder-home`) |
| Workspace | `<root>/workspace` |
| Temporary directory | `WUNDER_DESKTOP_TEMP_ROOT` |
| Desktop settings | `<temp>/config/desktop.settings.json` |
| Sessions and tool logs | local SQLite |

To move to a new machine, copy the whole install root (default `~/.wunder`) across. "Local first" means data stays on your machine; it doesn't affect reaching remote models or web pages.

On startup it scans the work tree (up to 4096 items), restores the state of unfinished tasks from last time, and clears session locks and leftover tasks.

## Interface layout

The main window has two switchable pages, Chat and Settings; everything else hangs off them as panels and dialogs.

### Chat page

- **Left column**: the task and thread tree, the **working-directory area** (local file tree + usage bar + **cloud workspaces**), and the settings entry at the bottom
- **Right column**: the top bar (workspace / thread name) + timeline + composer
- With no thread selected, a welcome page shows; file preview and online editing pop out as overlays without changing the two-column structure
- One agent can run several **independent task threads** in parallel, and workspaces can be **multiple**: local + cloud + remote device workspaces reached through device interlink

### Settings page

**12 categories**: General, Account, Model, Tools, Agents, Personas, Scheduled, Memory, Channels, Prompt packs, Archive, Help.

The category navigation supports search filtering. There is **no "device interlink" category**.

## First run

### Launch preparation

On launch, a lightweight splash screen appears first while local working directories, configuration, and sessions are prepared. The window appearing doesn't mean sessions are ready; you can start working once the workbench opens. First launches or slower devices may take longer to prepare.

### Model configuration

Settings → Model → Add:

- API Key
- Endpoint URL (a neutral endpoint such as `https://example.com/v1`)
- Model name and parameters (temperature, context length, and so on)

Confirm with "Test connection" before saving. This is also where you hook up TTS, image, and video models; **TTS can currently only be configured — there is no playback implementation**.

### Account profile

Settings → Account: username, email, avatar; set a login password if you plan to use it long-term.

### First conversation

Back on the chat page, type:

```
List the files in the current working directory
```

When you see the model think → a batch of tools → the final reply landing in the chat area, you're up and running.

## Capabilities unique to the Pupa

None of these exist on the Beehive, the Bridge, or the Helm:

- **Interactive terminal** (winpty on Windows, PTY on Linux)
- **Screenshot capture** (X11 on Linux, Win32 APIs on Windows)
- **Tray and system notifications**
- **Global hotkeys**
- **Voice input**

## Device interlink surfaces

Only **two** interlink surfaces are available in the Pupa:

1. The **cloud workspaces** in the sidebar's working-directory area: when the network is unavailable they clearly show "offline" alongside a "connect" button
2. The **remote command approval dialog**: high-risk commands (L3) in device interlink pop up for confirmation with a red border, showing the command level, risk, and source, plus a "don't remember" hint

The Settings page still holds a set of device-interlink options, but they aren't wired to any interface; **that entry point is not open yet**.

## Resetting the working state

When sessions, subagents, or workspaces get into a bad state, use "Settings → General → Reset workspace state":

- **It does**: stop running sessions, clear queued tasks, clean up session locks
- **It keeps**: skills, knowledge bases, long-term memory, user configuration, workspace files, session history

It **clears working state**; it is not a factory reset.

## Interaction constraints

Two default behaviors are worth separating:

- **The session queue is on by default**: sending another message during a run queues it instead of rejecting it as "user busy." Turn it off and messages sent during a run are bounced outright without entering the queue bar.
- **The subagent activity indicator**: when a task has spawned subagents, the chat area shows the child runs that are still working; subagents are temporary work units the main agent creates, reclaimed once the task ends.
