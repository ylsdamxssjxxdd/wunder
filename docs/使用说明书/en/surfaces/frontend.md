---
title: Hive Interface
summary: Hive is your workbench. Left navigation, middle list, right workspace — all daily work happens here.
---

# Hive Interface

Hive is your workbench. Conversations, file management, agent configuration, tool usage — it all happens here. Individuals open it via the [desktop app](/docs/en/start/desktop/); team members open it in a web browser. The interface and capabilities are identical.

## Three-column layout

- **Left column**: navigation (Chat, Files, Agents, Tools, Scheduled Tasks, Settings, Help)
- **Middle column**: lists (Session list, File list, Agent list, etc.)
- **Right column**: workspace (Chat, File preview, Settings panel, etc.)

## Chat

### Input area

Use the input area to start tasks, add context, and control the current thread. Common controls include:

| Area | Purpose |
|------|---------|
| **Message input** | Enter natural-language tasks, questions, or slash commands |
| **Attachment entry** | Select or drag local files; after processing, they are sent with the message |
| **Send / stop** | Send when idle, or stop the current execution while running |
| **Reasoning effort** | Brain icon filled with orange from the bottom: empty for no thinking, half-full for medium, full for highest; stays gray when following the model default, which does not reflect the actual thinking budget |
| **Work threads** | Each running entry has a light animation. You can switch or create another thread while the original continues in the background |

The web worker-thread panel uses the same outer container and inner list as the working directory panel. The title keeps a permanent status icon with an activity count; with no active threads it shows a gray idle icon and 0. Clicking once shows only running, queued, or waiting-for-interaction threads; clicking again restores all threads and the previous scroll position. After all activity ends, the filter button stays so you can leave the filter at any time; switching agents restores all threads. Drag-to-reorder still works while filtering.

For tasks that need existing files, put the files in the current agent's working directory first, then mention the path or file name in your message.

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
- **Orchestration mode**: orchestration threads are read-only in the chat page; go to the orchestration page to operate

### Automatic retries

If the model generates an invalid tool call or no usable content, the message status shows the reason, attempt count, and waiting time. You can stop the task during recovery. Normal progress resumes after recovery, and loading stops when the turn fails or is stopped. Refreshing or reopening the thread also restores recorded retry status.

## Orchestration workspace

The orchestration page is for swarm tasks that need continuous advancement:

- Queen bee continuously coordinates multiple worker bees
- View messages, status, and artifacts by round
- Continue from old rounds and generate new branches

Compared to the regular swarm page, the orchestration page emphasizes "continuous advancement" rather than one-time collaboration.

See [Orchestration](/docs/en/surfaces/orchestration/) for details.

## Files

The Files page shows the working directory available to the current agent. You can:

- Browse the working directory structure
- Upload, download, delete, and rename files
- Preview text, images, PDF, and other formats
- Put materials for the agent into the working directory, then reference file names or paths in chat

## Agents

- View all your agents
- Configure model, tools, prompts
- Add dedicated memory
- Each agent can have multiple threads

## Tools & skills

- View available tools list
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

- Embedded docs access without leaving Hive

## Status indicators

Watch for status indicators in the interface:

- 🔄 Running = Currently executing
- ⏳ Waiting = Needs your input or approval
- ✅ Complete = Ready to continue

## Further reading

- [Desktop Guide](/docs/en/start/desktop/)
- [Desktop Interface](/docs/en/surfaces/desktop-ui/)
- [Orchestration](/docs/en/surfaces/orchestration/)
- [Troubleshooting](/docs/en/help/troubleshooting/)
