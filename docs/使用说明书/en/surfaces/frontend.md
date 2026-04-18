---
title: User Frontend
summary: The user-side frontend lives in `frontend/`, unified into a single Messenger workbench that covers profile, chat, swarm, workspace, and system settings.
read_when:
  - You want to understand the primary interface that Wunder exposes to regular users
  - You need to know how chat, profile, swarm, files, and settings are organized
source_docs:
  - docs/API文档.md
  - frontend/src/views/MessengerView.vue
  - frontend/src/components/messenger/MessengerSettingsPanel.vue
  - frontend/src/components/messenger/DesktopRuntimeSettingsPanel.vue
updated_at: 2026-04-10
---

# User Frontend

Wunder's user-side frontend is not a single chat box — it is a unified messaging workbench.

It places all of the following inside a single Messenger shell:

- Agent chat
- My profile and account information
- Swarm collaboration
- Files and workspace
- Tool and knowledge configuration
- System settings and help documentation

## Code Location

- `frontend/`

## Current Main Entry Points

- `/app/chat`
- `/app/profile`
- `/app/workspace`
- `/app/tools`
- `/app/settings`
- `/app/channels`
- `/app/cron`

These entry points share the same outer framework — they are not independent products.

## How the Interface Is Organized

The current layout is a three-column structure:

- **Left column**: Primary navigation
- **Middle column**: List of conversations, agents, swarms, contacts, or resources
- **Right column**: The actual workspace

The right column hosts chat history, workflows, profile pages, settings panels, and the embedded help manual.

## Login, Registration, and Passwords

The login page now offers three common entry points:

- Sign in
- Register an account
- Reset password

The `Reset password` button is located to the right of `Register an account`. Once expanded, it only requires three fields:

- Username
- Email
- New password

If you are already logged in, you can update the following from "My Profile -> Edit Profile":

- Change username
- Change email
- Change login password

When changing your password, the frontend requires:

- Enter current password
- Enter new password
- Confirm new password

If you are only changing your username or email, the password fields can be left blank.

## What You Can See in My Profile

"My Profile" has been merged into the Messenger settings panel and no longer renders as a standalone page.

The profile section currently displays:

- Avatar, username, user ID, affiliated organization
- Level badge
- Experience progress bar
- Session count, 7-day activity, tool call count, agent count, cumulative tokens
- Token account ring chart

There are two key changes related to levels:

- The level badge has replaced the "full account" text that used to appear in the avatar area
- The experience bar is placed at the bottom of the profile card area, not at the bottom of the entire page

The Token account display has also been changed to a ring chart — the center shows the current balance, and the outer ring is segmented by balance health; below it, cumulative earnings, cumulative spending, and daily issuance are displayed.

## What the Chat Area Supports

The chat input area is a context assembler, not a plain text box.

- **Text input**: Supports regular questions, as well as `/new`, `/stop`, `/compact`, `/help`
- **File upload**: Supports selecting files via the paperclip button and drag-and-drop upload
- **Images**: Used directly as multimodal context
- **Documents**: Converted to Markdown text first
- **Audio**: Transcribed after upload
- **Video**: Frame extraction at `1 FPS` by default, with manual re-extraction available

Sending is blocked while attachments are still being processed. This is a safeguard, not a frozen UI.

If the current hive is in active orchestration mode, the orchestration thread is also protected in normal chat. You can inspect it, but continuing that workflow must happen on the orchestration page.

## Why the "New Thread" Button Is Sometimes Disabled

The chat page currently disables the `New Thread` button while an agent is running.

This is intentional, not a bug. The purpose is to prevent:

- Switching away before the running main thread finishes
- Conversation state and workflow becoming desynchronized
- The same agent competing for the main thread in the foreground simultaneously

To start a new thread, do one of the following first:

- Wait for the current run to complete naturally
- Stop the current conversation first, then create a new thread

## How to Read Swarm Status

Swarm page middle column entries are now aligned with the message list:

- As long as a swarm still has a running mission, the list item will show a pulsing highlight

The right panel and canvas workflow display also have a clearer division of labor:

- Queen bee nodes prioritize showing real tool traces
- Worker bee nodes continuously follow their own workflow updates
- Sub-agent nodes retain their completed tool traces after finishing

In other words, the swarm workflow area now focuses on "what tools were called and which step was reached," rather than just stacking statuses, session IDs, or summaries.

## Orchestration Workbench

The user frontend also includes a dedicated orchestration page in addition to normal chat and the swarm page.

It is the right place when:

- The queen needs to coordinate workers across multiple user rounds
- You want message, situation, and artifact snapshots per queen user round
- You want to continue from an older round and create a branch instead of overwriting history

Compared with the standard swarm page, the orchestration page treats the workflow as one persistent run rather than a one-off dispatch.

Important behavior:

- A new orchestration run creates fresh orchestration main threads for the queen and workers
- Later rounds stay on the same orchestration threads
- The timeline is based on the queen's user rounds
- Older rounds are viewed as snapshots by default
- Sending from an older round creates a new branch automatically

For the full operational guide, see:

- [Orchestration](/docs/en/surfaces/orchestration/)

## Dangerous Operations in System Settings

The system settings page now provides a "one-click reset work state" entry point.

It handles the current user's work state in a unified manner:

- Abort running conversations
- Clear queued tasks
- Terminate swarm runs
- Rebuild new main threads for the default agent and each user agent
- Clean up the work state directory

This is suitable for the following scenarios:

- A conversation is stuck
- Thread state and frontend display are clearly out of sync
- You want to return to a clean state after an abnormal swarm run

## Help Manual Entry

The user-side frontend now has a built-in documentation entry — no need to leave the current workbench.

- **Location**: Left column "More" -> "Help Manual"
- **Opens as**: Right panel embedded `/docs/?embed=user`

## When to Check This Page First

This page is useful when:

- You are changing user experience or page structure
- You need to decide whether a capability belongs in the chat area, profile area, or settings area
- You want to confirm what users can actually see in the frontend

## Further Reading

- [Getting Started with Desktop](/docs/en/start/desktop/)
- [Orchestration](/docs/en/surfaces/orchestration/)
- [Desktop Local Mode](/docs/en/ops/desktop-local-mode/)
- [Swarm Collaboration](/docs/en/concepts/swarm/)
- [Troubleshooting](/docs/en/help/troubleshooting/)
