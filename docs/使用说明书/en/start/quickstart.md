---
title: Quick Start
summary: Install and model configuration, the first message, how user turns and model turns are presented, plus the checks for a working setup and where to look after a failure.
read_when:
  - You are an individual user setting up wunder for the first time
  - You want the fastest path from install to a completed first task
source_docs:
  - crates/wunder-desktop/frontend/run.bat
  - crates/wunder-desktop/native.rs
  - crates/wunder-desktop/args.rs
updated_at: 2026-10-16
---

# Quick Start

Individual users start with the **Desktop**: download the matching build and it runs; there is no Server to deploy first. The four sections below follow install, model configuration, your first message, and the run itself.

## Download and install

Download the matching installer from [Releases](https://github.com/ylsdamxssjxxdd/wunder/releases). The Desktop targets 32-bit Win7 and Ubuntu 18.04 and also covers modern Windows / Linux.

- **Windows 7 32-bit**: download the `wunder-desktop-*-win7-x86-setup.exe` installer. It ships with a complete runtime; compatibility constraints limit some third-party features.
- **Linux x64 / arm64**: download the `wunder-desktop-*-linux-*.run` installer. It includes the Desktop, the `wunder-cli`, and the runtime; it needs neither FUSE, `libfuse2`, nor administrator rights, and installs directly on fuse3-only systems such as Ubuntu 24.04.
- **Android**: download the `wunder-android-*-arm64.apk` and install it.

Install it, then launch.

In a development environment you can also run the launch script directly (on Windows that's `run.bat`, or run `wunder-desktop` straight). It calls the engine natively in-process — it takes no local port and needs no local HTTP/WebSocket relay.

## Configure the model

Open "Settings → Model" and fill in your API Key and endpoint URL (in the form `https://example.com/v1`); verify once with "Test connection" before saving. Fill in the model name and parameters per your provider's guidance.

Data stays on your machine: local SQLite by default, with the configuration directory at `~/.wunder`; model requests go to the remote endpoint you configured.

## Send your first message

Back on the chat page, type:

```
List the files in the current working directory
```

and press Enter to send.

## The run

A run is shown by **user turn / model turn**:

- User turn = counted once for each message you send
- Model turn = **one model call** in the engine's main loop (which may include a batch of tool calls); tool calls don't count as a separate model turn

When the timeline shows tool batches, a "work process" entry for each turn, and a final reply, the run is finished.

## Capabilities available locally

- Local first: data stays on your machine and can work offline; the network is used only to reach models and web pages
- Desktop control: a desktop controller and desktop monitor (requires a vision model)
- Interactive terminal: winpty-based on Windows, PTY-based on Linux
- Screenshot capture, voice input, tray, system notifications, and global hotkeys
- One agent can have **several independent task threads** at once, and workspaces can be multiple too (local + cloud + remote device workspaces reached through device interlink)
- Edit the agent directly: change the system prompt and tool set at any time

## Checks for a working setup

- You can start a conversation successfully
- You can see the intermediate steps and tool-call batches
- You can get the final result and copy it
- When you scroll up to review history, new output doesn't yank you back to the bottom
- When something fails, you know which page to check

## Where to look after a failure

| Symptom | Check first |
| --- | --- |
| Launch screen never finishes | [First-run troubleshooting](/docs/en/start/troubleshooting-first-run/) |
| Model request 401/403 | Settings → Model → Test connection; [Configuration reference](/docs/en/capabilities/config/) |
| Message during a run rejected as "user busy" | Only the Cloud build can hit this when the session queue is off; the Desktop uses the frontend local waiting queue |
| Interface stutters, messages don't refresh | [Troubleshooting](/docs/en/help/troubleshooting/) |

## Other forms

Android, the Server, the Cloud, and the CLI are chosen per scenario:

- **Android**: the Desktop's Android form — the local AI workbench on a phone; install the APK and go. See the [Form matrix](/docs/en/start/forms/).
- **The Server**: multi-user sharing, permission management, channel integration, and publishing and governing agent apps. See [Server deployment](/docs/en/start/server/).
- **The Cloud (web)**: the cloud AI workbench delivered with the Server, accessed in a browser; its pages are delivered by a separate web-service container, on 18001 by default. See [Cloud interface](/docs/en/surfaces/frontend/).
- **The CLI**: the manual local entry point for terminals and automation. See [CLI usage](/docs/en/start/cli/).
