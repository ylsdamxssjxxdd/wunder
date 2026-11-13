---
title: Desktop Interface
summary: The local AI workbench and the default entry for individual users. Its main window has only two pages, chat and settings, with 11 categories in settings; everything else appears as panels and dialogs, including desktop-only capabilities and entries not yet wired up.
read_when:
  - You use the Desktop desktop app and want to know where a capability lives
  - You cannot find the device-interconnection panel or TTS playback in the Desktop
source_docs:
  - crates/wunder-desktop/frontend/ui/theme.slint
  - crates/wunder-desktop/frontend/ui/main.slint
  - crates/wunder-desktop/frontend/ui/settings_page.slint
  - crates/wunder-desktop/frontend/src/native_interlink.rs
  - crates/wunder-desktop/native.rs
updated_at: 2026-11-08
---

# Desktop Interface

It runs entirely on your own computer: there is no separate backend port and no network relay, and the interface calls the local engine directly. It is the default entry for individual users.

The UI is radically minimal: it keeps only the information and actions the current task needs, and complex capabilities unfold into panels and dialogs on demand without piling on decoration or status detail.

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
  - The input area holds the model switch, reasoning level, attachments, a voice-input button (when available), a screenshot button (when available), and send / stop; the approval-mode control has been removed from the input area and the plus menu
  - Below each reply, "Work process" folds away intermediate thinking, tool batches, and interim output

Since Desktop v1.0.0 the frontend local waiting queue (`waiting_turns`) takes over: messages sent during a run wait locally and are sent when the current turn settles, with no engine-side queue state.

### Settings page

**11 categories** in all:

| Category | Description |
| --- | --- |
| General | Sending, appearance and runtime environment |
| Local runtime | Local toolchain and runtime environment |
| Account | Cloud account, sign-in state and usage |
| Models | Add / edit LLM, TTS, image, and video models; TTS can be configured but not played |
| Tools | Opens the tools dialog, grouped into built-in / MCP / skills / knowledge bases |
| Agent | Built-in agent prompt and tools |
| Companion | Companion avatar, scale and bubble hints; the desktop floating window is Windows-only |
| Scheduled tasks | Scheduled jobs and their runs |
| Memories | Long-term memory entries; add, edit or delete |
| Channels | Channel accounts, bindings and logs |
| Archived threads | Archived threads: review or delete |

"Help" is not in the category list; its entry is the help button on the General page.

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
- **Expert data / patch-diff card**
- **Screenshot overlay**: a full-screen capture is pasted back into the input area
- **Tray and system notifications**: background running and completion reminders
- **Remote command approval pop-up**: a high-risk confirmation for interconnection commands, with a prominent border, risk level, and source details, plus a "cannot be remembered" prompt

## Capabilities unique to the Desktop

The Cloud, the Admin, and the CLI have none of these:

| Capability | What it is |
| --- | --- |
| **Interactive terminal** | Run commands right inside it; the Cloud, the Admin, and the CLI have none |
| **Screenshot capture** | A full-screen capture pasted back into the input area |
| **Tray and system notifications** | Reminders while running in the background and when a task finishes |
| **Global hotkey** | Bring it up with a system-wide shortcut |
| **Voice input** | Speak into the microphone and get text |
| **Local files and desktop control** | Read and write local files and operate the desktop; needs vision capability |

## Entries not wired up yet

- The **Device interconnection** category in settings is not wired up: the entry exists in code but never renders, so it cannot be opened today. Interconnection in the Desktop lives in only two places — the cloud workspace under "Working directory" in the sidebar, and the remote-command approval pop-up.
- **Voice output (TTS) is not implemented**: TTS models can be configured in settings, but there is no playback; the voice side is input-only. For spoken output, use the Cloud.
- **The early desktop shells are no longer maintained**: the Electron / Tauri frontend shells receive no feature updates. This does not affect the Desktop's underlying engine.

## Resetting the working state (no UI entry)

Since Desktop v1.0.0 the Settings page no longer exposes a reset entry; the backend capability remains available through `POST /wunder/auth/me/reset_work_state` (or the compatible `POST /wunder/desktop/reset_work_state`).

It stops running sessions, clears queued tasks, and cleans up session locks and state.

It keeps skills, knowledge bases, long-term memory, user configuration, workspace files, and session history.

It cleans up the working state rather than performing a factory reset.

## Differences from the Cloud

| Item | Desktop | Cloud |
| --- | --- | --- |
| Running mode | A desktop app installed on your own computer | Reached in a browser, in the cloud |
| Data storage | Local (`~/.wunder`) | Server storage |
| Interactive terminal | ✅ Yes | ❌ No |
| Voice input / output | Input ✅, output ❌ | Both directions ✅ |
| Tray / system notifications / global hotkey / screenshot | ✅ | ❌ |
| Device-interconnection settings panel | Category not opened; only the cloud workspace + approval pop-up | ✅ Devices category + interconnection view |
| One agent | Multiple threads, multiple workspaces | The same |
