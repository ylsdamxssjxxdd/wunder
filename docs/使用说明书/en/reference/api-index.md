---
title: Feature and Capability Quick Reference
summary: What wunder can do and where each capability lives, organized by feature area.
updated_at: 2026-10-13
---

Capabilities are listed by category below, each with its entry point.

## Chat

Location: the workbench (Honeycomb or Beehive) → the New task button in the left sidebar, or the thread list under the workspace.

- Ask questions and get answers
- Upload files for the agent to analyze
- Have the agent carry out tasks such as writing code, processing files, or searching for information
- Review chat history and pick up earlier conversations

Related: [Session Status Guide](/docs/en/reference/stream-events/), [Prompts and Personalization](/docs/en/reference/prompt-templates/)

## Working directory

Location: the workbench → the Working directory area below the workspace in the left sidebar.

- Upload documents, images, audio, and other files for the agent to use
- Download files the agent generates
- Browse and manage your single cloud directory
- See the files produced while the agent works

Related: [Workspace Routing and File Management](/docs/en/reference/workspace-routing/)

## Agent settings

Location: the workbench → Settings at the bottom of the left sidebar → Agent.

Every user has exactly one agent instance, generated from a one-to-one binding to an admin-side preset. There is no user-side entry point to create, delete, switch, or sort agents.

- View the current agent instance and the preset it is bound to
- Edit the fields the preset allows you to customize, such as the default model, approval mode, and tool set
- Set the agent's answer style (prompt template)
- Manage the agent's long-term memory

Related: [Prompts and Personalization](/docs/en/reference/prompt-templates/), [Tool System](/docs/en/concepts/tools/)

## Tools and skills

Location: the workbench → Settings at the bottom of the left sidebar → Tools. The user side only views and toggles; the tool catalog is maintained by admins.

- View the available tool list
- Configure skills (preset capability packs)
- Add knowledge bases so the agent can draw on your own material

The built-in tools cover:

- File operations: read, write, and search files
- Web operations: search and fetch page content
- Code operations: run commands and write code
- System operations: screenshots and system status
- More capabilities are added through MCP

Related: [Tool System](/docs/en/concepts/tools/), [Prompts and Skills](/docs/en/concepts/prompt-and-skills/)

## Channel access

Location: the Bridge → Channel management.

- Bind accounts on Feishu, WeCom, WeChat, QQ, and other platforms
- Let users on those external platforms reach the agent
- View channel status and message logs
- Troubleshoot channel-related problems

When members of an organization already work in Feishu or WeChat, channel access lets them use the agent without switching tools.

Related: [Channel Management Guide](/docs/en/ops/channel-runtime/), [Bridge Panel Guide](/docs/en/reference/admin-panels/)

## Scheduled tasks

Location: the workbench → Settings at the bottom of the left sidebar → Scheduled tasks.

- Create tasks that run on a schedule
- View execution history
- Manage, pause, and delete scheduled tasks

This suits recurring work such as a daily summary report or a periodic system check.

## System settings

Location: the workbench → Settings at the bottom of the left sidebar.

- Change the interface language
- Configure the service address to connect to
- Manage your personal account information
- View usage statistics and quota balance

Related: [System Settings Reference](/docs/en/reference/config/)

## Bridge features (administrators only)

Location: open the Bridge address in a browser. The main panels are:

- System settings: global configuration
- User management: user accounts and quota accounts
- Organization management: organizational structure
- Model configuration: the AI models you can choose
- Tool management: the available tool list
- Channel monitoring: external channel status
- Performance monitoring: how the system is running

Related: [Bridge Panel Guide](/docs/en/reference/admin-panels/), [System Settings Reference](/docs/en/reference/config/)

## Locate by need

| I want to… | Where to go |
|---------|--------|
| Start a conversation | New task in the left sidebar → just type |
| Upload a file for the agent to analyze | Use upload or drag files into the working directory area in the left sidebar, or add an attachment in the composer |
| See the files the agent generates | The working directory area in the left sidebar |
| Customize the agent's style | Settings at the bottom of the left sidebar → Agent → prompt template |
| Create a scheduled task | Settings at the bottom of the left sidebar → Scheduled tasks → create task |
| Manage users (administrator) | Bridge → User management |
| Configure channels (administrator) | Bridge → Channel management |

## Further reading

- [Core Concepts](/docs/en/concepts/)
- [Quick Start](/docs/en/start/quickstart/)
- [Troubleshooting](/docs/en/help/troubleshooting/)
