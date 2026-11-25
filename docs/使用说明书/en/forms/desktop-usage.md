---
title: Desktop Usage
nav_title: Usage
summary: Desktop usage: first launch, model setup, sending messages and reading a run, plus capabilities and boundaries.
updated_at: 2026-11-18
---

# Desktop Usage

## Usage

### First launch

- A lightweight splash page appears first while the working directory, configuration and sessions are prepared; the window being up does not mean the session is ready.
- Settings → Models → Add: API Key, endpoint (`https://example.com/v1` style), model name and parameters; verify with Test connection before saving.
- Settings → Account: user name, e-mail, avatar; set a login password if you plan to use it long term.
- Back on the chat page, send `List the files in the current working directory` and watch the tool batches, the process log and the final reply.

### Runs and turns

- The composer carries model switching, thinking level, attachments, voice input, screenshot and send / stop.
- Each reply has a Process section that folds the intermediate thinking, tool batches and intermediate output; the latest turn stays expanded.
- Sending another message while a turn runs queues it above the composer and sends it once the current turn settles.
- Trajectory, working directory, terminal view, tool management and subagent details are all separate panels, opened from the chat page.

### Shortcuts

The screenshot shortcuts only apply while the workbench window has focus, so `F1` / `F2` keep their meaning in other programs:

| Key | What it does |
| --- | --- |
| `F1` | Hide the workbench first, then open the region selector over what it covered |
| `F2` | Open the region selector with the workbench left in place |

The confirmed region drops into the composer as an attachment and is copied to the system clipboard as well. The tray menu and the composer's `+` → Screenshot entry are the same as `F1`.

The selection stays editable after you release the mouse: drag inside it to move, drag any of the eight corner/edge handles to resize, nudge with the arrow keys (1 screen pixel, 10 with `Shift`), then press `Enter` to confirm or `Esc`/right-click to cancel. Moving the pointer no longer outlines windows continuously; press `Space` to snap. It uses the real cursor position from when the selector opened, snaps to the window under it, then UI Automation refines the rectangle to a button, tab, list row or similar control in the background. Detection runs only on the key press, so opening the selector itself no longer enumerates windows or probes controls; pressing `Space` again re-detects at the new pointer position, and a failed refinement keeps the window rectangle. `F1` no longer waits a fixed settle: it hides the window, captures immediately and re-checks the frame, so the selector opens as quickly as with `F2`.

## Desktop-only capabilities

| Capability | What it is |
| --- | --- |
| Interactive terminal | Run commands in a real terminal view (winpty on Windows, PTY on Linux) |
| Screenshot capture | Press `F1` to hide the workbench and capture the region underneath, `F2` to capture with it in place; the result drops back into the composer and is also copied to the system clipboard. Both keys only apply while the workbench window has focus |
| Tray and notifications | Keep running in the background and get told when a task finishes |
| Global hotkey | Wake the window from anywhere |
| Voice input | Dictate instead of typing |
| Local files and desktop control | Read and write local files, drive the desktop (needs a vision model) |

## Boundaries

- **No device-interlink settings panel**: the 11 settings categories do not include it; the usable parts are the cloud workspaces in the sidebar's working directory and the remote-command approval dialog.
- **No TTS playback**: voice input works and TTS models can be configured, but nothing plays; use [Cloud](/docs/en/forms/cloud-ui/) for spoken output.
- **No UI entry for resetting the working state**: call `POST /wunder/auth/me/reset_work_state`; it stops running sessions, clears queued tasks and locks, and keeps skills, knowledge base, long-term memory, configuration and history.
- **A compatibility bridge port** (18123 by default) serves legacy clients only.
- **Smart selection is on demand**: window/control detection runs only when `Space` is pressed. The first press builds the window tree lazily, then UI Automation refines it. Self-drawn apps or apps with accessibility disabled may only resolve to the window; a cross-process timeout or failed refinement keeps the window rectangle and never blocks dragging or confirming.
- **Electron / Tauri shells are no longer maintained**; that does not affect the engine or the native desktop app.
