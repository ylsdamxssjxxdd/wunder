---
title: Beehive Interface
summary: The cloud AI workbench, provided with the Hull deployment and reached in a browser. Its settings include tools, memory, channels, scheduled tasks, devices, and archive; it supports two-way voice (TTS included) and has no interactive terminal.
read_when:
  - You use the Beehive in a browser and want to know where each capability lives
  - You are checking what the Beehive can do that the Pupa cannot (and vice versa)
source_docs:
  - frontend/src/router/index.ts
  - frontend/src/api/chat.ts
  - docker-compose-x86.yml
  - frontend/nginx.conf
  - crates/wunder-server/src/main.rs
updated_at: 2026-10-16
---

# Beehive Interface

The cloud AI workbench, provided with the Hull: once the Hull is deployed, members open it in a browser and start working. It has no process of its own — concretely, the built pages in `frontend/dist` are served by an nginx container (port 18001 by default); the Hull itself only serves the Bridge's static directory, not the Beehive.

## Access and sign-in

Open the deployed address in a browser. If you are not signed in you land on the login page, where you can register directly. After signing in you go straight to your agent's chat page.

- **Chat**: the main workspace, covered under "Main area"
- **Embedded chat**: a conversation embedded inside another page
- **External workflow landing pages**: pages for external flows reached by link
- **Settings**: you stay on the chat page, with the right side switching to the matching settings category
- **Thread trajectory**: the execution record of a whole thread
- **Admin console**: admins get separate user, agent, and system management pages after signing in

The Beehive and the Bridge are two separate interfaces: the Beehive faces users, the Bridge faces admins. The Beehive is itself a team / multi-user setting, where members sign in with a Hull account; for a single machine, the Pupa is recommended.

## Main area

The Beehive's main area is a three-column chat workspace:

- **Left column**: new task, search, workspace and thread tree, working directory, settings entry
- **Right column**: chat top bar + timeline + input area
- **Overlays**: file preview, inline editing, companion overlay

One agent can hold several independent task threads at once, and there can be several workspaces: your own cloud workspace (`/workspaces/<user>/workspace`) plus remote device workspaces reached through device interconnection.

## Realtime connectivity

All realtime traffic runs over **a single WebSocket** (`/chat/ws`), which the client multiplexes: new messages, execution progress, and approval prompts all arrive through it. The Beehive does not open several WebSocket channels at once. After a reconnect, the state is filled in by ordinary requests, so you do not have to reload the page.

If the network or a proxy blocks that one connection, realtime state stops updating; refreshing the page usually restores it.

## Rendering long conversations

The timeline does not lay out an entire thread at once; it loads by window as you scroll, so a long thread does not freeze when opened. Parsing and rendering also run in background workers, so they do not block the main thread.

The rendering optimization for extremely large threads is a rewritten plan whose production implementation is not fully in place. Very long threads can still stutter.

## Settings categories

The Beehive's settings hang off the chat page as categories and share the same backend capabilities as the Pupa. They include:

- **General / Account / Models / Agent / Companion / Prompt packs / Help** (the same as the Pupa)
- **Tools**: four tabs — built-in / MCP / skills / knowledge bases
- **Memory**: long-term memory management
- **Channels**: channel binding and status
- **Scheduled**: scheduled tasks
- **Devices**: entry to device-interconnection management; command approvals pop up inline in the timeline
- **Archive**: archived threads

## Input area

- **Message input**: natural language, plus slash commands
- **Attachments**: pick locally or drag and drop
- **Send / stop**: sends when idle, turns into stop while running
- **Model switch**: change models right in the input area, without jumping to settings
- **Approval mode**: Suggest / Auto-edit / Fully automatic (fully automatic by default)
- **Reasoning level**: the brain icon at the bottom; gray follows the model, while empty / half-full / full mean no thinking / medium / highest
- **Work-thread panel**: shares the same outer panel as the working directory; a status icon and activity count stay at the right of the title; the filter button remains after all activity ends, so you can leave the filter at any time, and drag-to-reorder still works while filtering

## Timeline and status

- Below each reply, "Work process" folds away intermediate thinking, tool batches, and interim output
- Status: 🔄 Running / ⏳ Waiting (needs your input or approval) / ✅ Complete
- Context usage and duration belong to the model request that issued the tool; tools running in parallel within one request share the same numbers and are not added up per entry
- Very fast calls may show `0ms`; failed and cancelled calls also record their duration

## Slash commands

Type them into the message box:

- `/new` — create a new thread
- `/stop` — stop the current execution
- `/compact` — compress the conversation on demand
- `/help` — open the help docs
- `/goal` — goal management

`/compact` also counts as one user turn; the compressed summary appears right in the reply bubble after the command and survives switching threads or refreshing the page.

## Protection mechanisms

- **Attachment processing**: the send button is disabled until processing finishes
- **Background running**: switching, creating, refreshing, or briefly going offline does not cancel running work threads; once you reconnect, status and pending approvals are restored from saved events
- **Empty-thread reuse**: a new thread prefers an empty thread the current agent has not used yet; threads that have sent messages, are running, or were spawned by subagents are never reused
- **Deletion sync**: after an admin deletes a thread, your side clears the entry and its cache on the next list refresh or switch; clicking a stale entry shows a notice and returns you to the input page

## Differences from the Pupa

| Item | Beehive | Pupa |
| --- | --- | --- |
| Running mode | Reached in a browser, in the cloud | A desktop app installed on your own computer |
| Data storage | Hull storage | Local |
| Interactive terminal | ❌ No | ✅ Yes |
| Voice input / output | Both directions ✅ (TTS included) | Input ✅, output ❌ |
| Device interconnection | ✅ Devices category + interconnection view | ⚠️ Cloud workspace + approval pop-up only; settings category not opened |
| Tray / system notifications / global hotkey / screenshot | ❌ | ✅ |
| Attachment upload limit | Temporary attachments 200 MB, persistent attachments 10 MB | Local files referenced directly |
