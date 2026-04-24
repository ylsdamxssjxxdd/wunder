---
title: Desktop Guide
summary: The first choice for individual users. Download, install, and start — no server needed.
read_when:
  - You want to get wunder running right away
  - You care more about the desktop workbench than deploying a full server
source_docs:
  - docs/API文档.md
updated_at: 2026-04-10
---

# Desktop Guide

If you want to start using it right away, don't set up a server first — just use Desktop.

Open it and you have a complete agent workbench. No server required.

## When to Choose Desktop

| Scenario | Choose Desktop |
|----------|---------------|
| Want to start immediately, no deployment | ✅ |
| Need to work with local files, windows, browsers | ✅ |
| Prefer a graphical interface | ✅ |
| Might connect to remote later, but start local first | ✅ |
| Need multi-user governance and admin backend | ❌ Choose Server |

## What You'll See

Desktop uses a unified three-column layout:

- **Left column**: Navigation (Chat, Files, Agents, Tools, Settings, Help)
- **Middle column**: Lists (Sessions, Files, Agent list, etc.)
- **Right column**: Workspace (Chat, File preview, Settings panel, etc.)

## 5 Steps to Get Started

### 1. Download and Install

Get the installer for your system from Releases, install and launch.

### 2. First Launch

It automatically creates local working and config directories. No manual setup needed.

### 3. Configure Model

Go to "System Settings" → "Model Configuration", enter:

- API Key
- Endpoint URL
- Model name

Click "Test Connection" before saving to confirm it works.

### 4. Set Up Your Profile

After first login, it's a good idea to:

- Go to "My Profile → Edit" to change your username or email
- Set a new login password if you plan to use it long-term

### 5. Start Your First Conversation

Go back to the chat page and type:

```
List the files in the current workspace
```

You'll see: model starts → tools work step by step → final reply appears in the chat area.

## Two Common Interaction Constraints

### Can't Create New Thread While Running

When the current agent is still running, the "New Thread" button is disabled. This is normal protection — wait for it to finish or stop the current session first.

### Swarm Running Has Visual Indicator

When a swarm is still active, its entry in the middle column will have a breathing highlight, reminding you it's still working.

## Workspace State Messed Up?

If your sessions, swarms, or workspace get into a bad state, use **"Reset Workspace State"** in System Settings.

It clears running states but does NOT delete your skills, knowledge bases, or other long-term assets.

## Local Mode Extras

In Desktop local mode, the chat area also shows:

- 🎤 Microphone button (voice input)
- 📷 Screenshot button (screen capture analysis)

Availability depends on your machine's permissions.

## Need Help? Open the Manual

Left column → More → Help Manual. You can browse docs without leaving the app.

## Next Steps

- Understand the interface: [User Interface](/docs/en/surfaces/frontend/)
- Understand local mode boundaries: [Desktop Local Mode](/docs/en/ops/desktop-local-mode/)
- See swarm collaboration: [Swarm Collaboration](/docs/en/concepts/swarm/)
- Running into issues: [Troubleshooting](/docs/en/help/troubleshooting/)
