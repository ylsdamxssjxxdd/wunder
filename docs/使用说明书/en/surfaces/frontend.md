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

More than just a text box:

| Capability | Description |
|------------|-------------|
| **Text input** | Regular questions or slash commands |
| **File upload** | Click upload or drag and drop |
| **Image upload** | Directly as multimodal input |
| **Document upload** | Automatically converted to Markdown |
| **Audio upload** | Automatically transcribed to text |
| **Video upload** | Frames extracted for analysis |

### Slash commands

| Command | Action |
|---------|--------|
| `/new` | Create new thread |
| `/stop` | Stop current execution |
| `/compact` | Manually compress conversation |
| `/help` | Open help docs |

### Protection mechanisms

- **Attachment processing**: send button disabled until processing completes
- **Thread running**: new thread button disabled until current one finishes or is stopped
- **Orchestration mode**: orchestration threads are read-only in the chat page; go to the orchestration page to operate

## Orchestration workspace

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
- Token account balance and trends

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
