---
title: Desktop Usage
nav_title: Usage
summary: Desktop usage: first launch, model setup, sending messages and reading a run, plus capabilities and boundaries.
updated_at: 2026-11-14
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

## Desktop-only capabilities

| Capability | What it is |
| --- | --- |
| Interactive terminal | Run commands in a real terminal view (winpty on Windows, PTY on Linux) |
| Screenshot capture | Capture the screen and drop it back into the composer |
| Tray and notifications | Keep running in the background and get told when a task finishes |
| Global hotkey | Wake the window from anywhere |
| Voice input | Dictate instead of typing |
| Local files and desktop control | Read and write local files, drive the desktop (needs a vision model) |

## Boundaries

- **No device-interlink settings panel**: the 11 settings categories do not include it; the usable parts are the cloud workspaces in the sidebar's working directory and the remote-command approval dialog.
- **No TTS playback**: voice input works and TTS models can be configured, but nothing plays; use [Cloud](/docs/en/forms/cloud-ui/) for spoken output.
- **No UI entry for resetting the working state**: call `POST /wunder/auth/me/reset_work_state`; it stops running sessions, clears queued tasks and locks, and keeps skills, knowledge base, long-term memory, configuration and history.
- **A compatibility bridge port** (18123 by default) serves legacy clients only.
- **Electron / Tauri shells are no longer maintained**; that does not affect the engine or the native desktop app.
