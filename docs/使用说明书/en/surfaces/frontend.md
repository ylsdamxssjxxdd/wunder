---
title: Beehive Interface
summary: Beehive is your workbench. Two columns: the left sidebar holds new task, search, the workspace thread tree, the working directory, and settings; the right side is chat.
---

# Beehive Interface

Beehive is your workbench. Conversations, file handling, and settings all happen here. Individuals open it via the [desktop app](/docs/en/start/desktop/); team members open it in a web browser.

## Two-column layout

```
┌──────────────┬─────────────────────────────────────┐
│  Sidebar     │  Chat header (workspace / thread)   │
│  (240px)     ├─────────────────────────────────────┤
│  New task    │                                     │
│  Search      │  Timeline                           │
│  ─ Workspace ─                                     │
│   Thread A   │                                     │
│   Thread B   ├─────────────────────────────────────┤
│  ─ Working dir ─  Composer (model / send)          │
│  File tree   │                                     │
│  Usage bar   │                                     │
│  Settings    │  Status bar                         │
└──────────────┴─────────────────────────────────────┘
```

- **Left sidebar** (fixed 240px): new task button, search box, workspace and thread tree, the **working directory** area (file tree + toolbar + usage bar), and the settings entry at the bottom
- **Right column** (fluid): the welcome page when no thread is selected, otherwise the chat page (header + timeline + composer)

The two sidebar halves scroll independently, and the divider between them can be dragged; on narrow screens the sidebar becomes a drawer overlay.

## Chat

### Input area

Use the input area to start tasks, add context, and control the current thread. Common controls include:

| Area | Purpose |
|------|---------|
| **Message input** | Enter natural-language tasks, questions, or slash commands |
| **Attachment entry** | Select or drag local files; after processing, they are sent with the message |
| **Send / stop** | Send when idle, or stop the current execution while running |
| **Model switch** | Switch the active model right in the composer instead of jumping to settings |
| **Approval mode** | Choose how strictly tool execution is approved (a cloud security boundary) |
| **Reasoning effort** | Brain icon filled with orange from the bottom: empty for no thinking, half-full for medium, full for highest; stays gray when following the model default, which does not reflect the actual thinking budget |
| **Work threads** | Each running entry has a light animation. You can switch or create another thread while the original continues in the background |

The web worker-thread panel uses the same outer panel and inner list as the working directory panel. The title keeps a permanent status icon with an activity count; with no active threads it shows a gray idle icon and 0. Clicking once shows only running, queued, or waiting-for-interaction threads; clicking again restores all threads and the previous scroll position. After all activity ends, the filter button stays so you can leave the filter at any time. Drag-to-reorder still works while filtering.

For tasks that need existing files, put the files in the working directory area of the left sidebar first, then mention the relative path or file name in your message; you can also use "Reference to chat" from a file row menu to pull it into the composer.

### Context and tool timing

The input area's "used / capacity" shows the input tokens of the most recent model request. While output is streaming it keeps the last confirmed value, and updates after the next request is observed; after compaction it may drop or reset, and a brief gap in statistics will not flash empty.

In the agent loop, the context and token usage belong to the model request that issued that tool. Multiple parallel tools from the same request share these numbers — do not add them up per entry. Timing shows how long the tool took (including approval waits during processing); very fast calls may show `0ms`, and failed or cancelled calls also record their duration.

### Slash commands

| Command | Action |
|---------|--------|
| `/new` | Create new thread |
| `/stop` | Stop current execution |
| `/compact` | Manually compress conversation |
| `/help` | Open help docs |

`/compact` counts as a user turn. Its summary appears immediately in the following assistant bubble, with progress in the agent loop and no separate divider. Switching threads or refreshing preserves the summary and completed status.

### Protection mechanisms

- **Attachment processing**: send button disabled until processing completes
- **Background execution**: switching, creating, refreshing, or temporarily going offline does not cancel existing work threads. Reopening restores status and pending approvals from persisted events
- **Empty thread reuse**: clicking "New thread" first opens an unused empty thread that already belongs to the current agent; clicking again after switching to another thread reuses the same one, avoiding piles of duplicate new sessions. Threads that already sent messages, are running, or were spawned by subagents are never reused as empty threads.
- **Deletion sync**: after an administrator deletes a thread, the client cleans up the entry and its cache on the next list refresh or switch, so you never need to clear browser data manually. If you click a stale entry that has not refreshed yet, the page shows a notice and returns to that agent's input page instead of leaving an unopenable thread.

### Automatic retries

If the model generates an invalid tool call or no usable content, the message status shows the reason, attempt count, and waiting time. You can stop the task during recovery. Normal progress resumes after recovery, and loading stops when the turn fails or is stopped. Refreshing or reopening the thread also restores recorded retry status.

## Working directory

The "Working directory" area below the workspace tree in the left sidebar shows your single cloud directory. You can:

- browse the directory structure (directories expand lazily)
- upload, download, delete, rename, move, and copy files
- preview text, images, PDF, Office documents, and diagrams
- multi-select to batch delete or download an archive
- use "Reference to chat" in a file row menu to pull a file into the composer
- see used capacity and file count at the bottom of the area

## Agent settings

Every user has exactly one agent instance, generated from a 1:1 admin-side preset binding. There is no user-side entry point to create, delete, switch, or sort agents. In **Settings** at the bottom of the left sidebar you can:

- view the current agent instance and its bound preset
- edit the fields the preset allows you to customize (default model, approval mode, tool set, and so on)
- configure prompt templates and skills
- add dedicated memory
- the agent can still own multiple independent threads

Fields the preset does not open for customization are shown as "set by admin" and stay read-only.

## Tools & skills

- View the available tools list (user-side is read-only plus toggles; the catalog is maintained by admins)
- Configure skills (capability packages)
- Add knowledge bases for agent reference

## Scheduled tasks

- Create scheduled tasks
- View execution history
- Manage, pause, delete tasks
- Manually trigger an execution

## My profile & settings

### My profile

- Avatar, username, level
- Experience progress bar
- Usage statistics (sessions, tool calls, Token consumption)
- Quota account balance and trends

### Account management

- Change username, email, password
- View organization membership

### System settings

- Interface language switching
- Reset workspace state (for recovery)

### Help manual

- Embedded docs access without leaving Beehive

## Status indicators

Watch for status indicators in the interface:

- 🔄 Running = Currently executing
- ⏳ Waiting = Needs your input or approval
- ✅ Complete = Ready to continue

## Further reading

- [Desktop Guide](/docs/en/start/desktop/)
- [Desktop Interface](/docs/en/surfaces/desktop-ui/)
- [Troubleshooting](/docs/en/help/troubleshooting/)
