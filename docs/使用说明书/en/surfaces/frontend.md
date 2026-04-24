---
title: User Interface
summary: Your main workbench. Left navigation, middle list, right workspace — all daily work happens here.
---

# User Interface

The user interface is your main workbench. Conversations, file management, agent configuration, tool usage — it all happens here.

## Three-Column Layout

- **Left column**: Navigation (Chat, Files, Agents, Tools, Scheduled Tasks, Settings, Help)
- **Middle column**: Lists (Session list, File list, Agent list, etc.)
- **Right column**: Workspace (Chat, File preview, Settings panel, etc.)

## Chat

### Input Area

More than just a text box:

| Capability | Description |
|------------|-------------|
| **Text input** | Regular questions or slash commands |
| **File upload** | Click upload or drag and drop |
| **Image upload** | Directly as multimodal input |
| **Document upload** | Automatically converted to Markdown |
| **Audio upload** | Automatically transcribed to text |
| **Video upload** | Frames extracted for analysis |

### Slash Commands

| Command | Action |
|---------|--------|
| `/new` | Create new thread |
| `/stop` | Stop current execution |
| `/compact` | Manually compress conversation |
| `/help` | Open help docs |

### Protection Mechanisms

- **Attachment processing**: Send button disabled until processing completes
- **Thread running**: New thread button disabled until current one finishes or is stopped
- **Orchestration mode**: Orchestration threads are read-only in the chat page; go to the orchestration page to operate

## Orchestration Workbench

The orchestration page is for swarm tasks that need continuous advancement:

- Queen bee continuously coordinates multiple worker bees
- View messages, status, and artifacts by round
- Continue from old rounds and generate new branches

Compared to the regular swarm page, the orchestration page emphasizes "continuous advancement" rather than one-time collaboration.

See [Orchestration](/docs/en/surfaces/orchestration/) for details.

## Files

- Browse directory structure
- Upload, download, delete files
- Preview text, images, PDF, and other formats

## Agents

- View all your agents
- Configure model, tools, prompts
- Add dedicated memory
- Each agent can have multiple threads

## Tools & Skills

- View available tools list
- Configure skills (capability packages)
- Add knowledge bases for agent reference

## Scheduled Tasks

- Create scheduled tasks
- View execution history
- Manage, pause, delete tasks
- Manually trigger an execution

## My Profile & Settings

### My Profile

- Avatar, username, level
- Experience progress bar
- Usage statistics (sessions, tool calls, Token consumption)
- Token account balance and trends

### Account Management

- Change username, email, password
- View organization membership

### System Settings

- Interface language switching
- Reset workspace state (for recovery)

### Help Manual

- Embedded docs access without leaving the workbench

## Status Indicators

Watch for status indicators in the interface:

- 🔄 Running = Currently executing
- ⏳ Waiting = Needs your input or approval
- ✅ Complete = Ready to continue

## Further Reading

- [Desktop Guide](/docs/en/start/desktop/)
- [Orchestration](/docs/en/surfaces/orchestration/)
- [Troubleshooting](/docs/en/help/troubleshooting/)
