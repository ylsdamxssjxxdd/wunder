---
title: Pupa Interface
summary: The local AI workbench and the default entry for individual users. Its main window has only two pages, chat and settings, with 12 categories in settings; everything else appears as panels and dialogs, including desktop-only capabilities and entries not yet wired up.
read_when:
  - You use the Pupa desktop app and want to know where a capability lives
  - You cannot find the device-interconnection panel or TTS playback in the Pupa
source_docs:
  - frontend-slint/ui/theme.slint
  - frontend-slint/ui/main.slint
  - frontend-slint/ui/settings_page.slint
  - frontend-slint/src/native_interlink.rs
  - crates/wunder-desktop/native.rs
updated_at: 2026-10-16
---

# Pupa Interface

It runs entirely on your own computer: there is no separate backend port and no network relay, and the interface calls the local engine directly. It is the default entry for individual users.

## Starting and running

Open the desktop app: there is no port to specify and no required parameters.

- Data root: `~/.wunder`; the `WUNDER_HOME` environment variable moves it elsewhere
- Workspace: `~/.wunder/workspace`
- Desktop settings: `<temp>/config/desktop.settings.json`
- Sessions and tool logs: a local database
- A separate compatibility port (18123 by default) serves a few older clients only; day-to-day use never touches it

"Local-first" means the data stays on this machine; it does not affect access to remote models or web pages.

Moving to another computer: copy the whole `~/.wunder` folder (workspace, database, and settings). On the first launch it scans the work tree and restores the state of tasks that were queued, waiting, running, or cancelling.

## Main window structure

The main window has two switchable pages, **chat** and **settings**; every other capability hangs off them as panels and dialogs.

### Chat page

Two columns:

- **Left column**: tasks and thread tree, working directory, settings entry
  - The working directory shows both the local work tree and the **cloud workspace**
  - One agent can hold **several independent task threads** at once, and there can be **several** workspaces: the local directory, a cloud workspace, and remote device workspaces reached through device interconnection; locks and realtime state are isolated per thread
- **Right column**: top bar (workspace / thread name) + timeline + input area
  - The welcome page shows when no thread is selected
  - The input area holds the model switch, approval mode, reasoning level, attachments, a voice-input button (when available), a screenshot button (when available), and send / stop
  - Below each reply, "Work process" folds away intermediate thinking, tool batches, and interim output

With the session queue on, messages sent to a new thread during a run queue up instead of being rejected as "user busy".

### Settings page

**12 categories** in all:

| Category | Description |
| --- | --- |
| General | Interface language, appearance, startup behavior |
| Account | Username, email, password, avatar |
| Models | Add / edit LLM, TTS, image, and video models; TTS can be configured but not played |
| Tools | Opens the tools dialog, grouped into built-in / MCP / skills / knowledge bases |
| Agent | System prompt, default approval mode, default skill set |
| Companion | Companion-package choice and floating-window size; the desktop floating window is Windows-only |
| Scheduled | Entry to scheduled tasks |
| Memory | Long-term memory management and hits |
| Channels | Channel binding and status |
| Prompt packs | Prompt packs available in-app, with language defaults |
| Archive | Archived thread list |
| Help | Embedded docs and version number |

The category bar supports search filtering; when nothing matches it shows an empty state rather than a blank area.

## Panels and dialogs

These areas live inside the two pages, not as pages of their own:

- **Sidebar / timeline / input area**: the three parts of the chat page
- **Terminal view**: an interactive command session where you type just as in a terminal
- **Tools dialog**: grouped into built-in / MCP / skills / knowledge bases
- **Companion dialog**: companion-package list and preview
- **Workspace dialog**: switch to or create a workspace
- **Working directory**: file tree + toolbar + usage bar + cloud workspace
- **Thread trajectory**: the execution ledger of a whole thread (it replaces the old log dialog)
- **Subagent details**
- **Channels page**: channel management on the user side
- **Scheduled page**: scheduled tasks on the user side
- **Activity card / expert data / patch-diff card**
- **Screenshot overlay**: a full-screen capture is pasted back into the input area
- **Tray and system notifications**: background running and completion reminders
- **Remote command approval pop-up**: a high-risk confirmation for interconnection commands, with a prominent border, risk level, and source details, plus a "cannot be remembered" prompt

## Capabilities unique to the Pupa

The Beehive, the Bridge, and the Helm have none of these:

| Capability | What it is |
| --- | --- |
| **Interactive terminal** | Run commands right inside it; the Beehive, the Bridge, and the Helm have none |
| **Screenshot capture** | A full-screen capture pasted back into the input area |
| **Tray and system notifications** | Reminders while running in the background and when a task finishes |
| **Global hotkey** | Bring it up with a system-wide shortcut |
| **Voice input** | Speak into the microphone and get text |
| **Local files and desktop control** | Read and write local files and operate the desktop; needs vision capability |

## Entries not wired up yet

- The **Device interconnection** category in settings is not wired up: the entry exists in code but never renders, so it cannot be opened today. Interconnection in the Pupa lives in only two places — the cloud workspace under "Working directory" in the sidebar, and the remote-command approval pop-up.
- **Voice output (TTS) is not implemented**: TTS models can be configured in settings, but there is no playback; the voice side is input-only. For spoken output, use the Beehive.
- **The early desktop shells are no longer maintained**: the Electron / Tauri frontend shells receive no feature updates. This does not affect the Pupa's underlying engine.

## Resetting the working state

When a session is stuck in the running state, a subagent's status is inconsistent, or the interface does not match reality, use **Settings → General → Reset working state**.

It stops running sessions, clears queued tasks, and cleans up session locks and state.

It keeps skills, knowledge bases, long-term memory, user configuration, workspace files, and session history.

It cleans up the working state rather than performing a factory reset.

## Differences from the Beehive

| Item | Pupa | Beehive |
| --- | --- | --- |
| Running mode | A desktop app installed on your own computer | Reached in a browser, in the cloud |
| Data storage | Local (`~/.wunder`) | Hull storage |
| Interactive terminal | ✅ Yes | ❌ No |
| Voice input / output | Input ✅, output ❌ | Both directions ✅ |
| Tray / system notifications / global hotkey / screenshot | ✅ | ❌ |
| Device-interconnection settings panel | Category not opened; only the cloud workspace + approval pop-up | ✅ Devices category + interconnection view |
| One agent | Multiple threads, multiple workspaces | The same |
