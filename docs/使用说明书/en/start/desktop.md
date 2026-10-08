---
title: Desktop Guide
summary: wunder's default form. Download, install, and start — no server needed.
read_when:
  - You want to get wunder running right away
  - You care more about Beehive's desktop form than deploying a full server
source_docs:
  - docs/API文档.md
  - frontend/src/components/messenger/DesktopRuntimeSettingsPanel.vue
  - frontend/src/components/messenger/MessengerSettingsPanel.vue
---

# Desktop Guide

The desktop app is wunder's default form. Download and install it, and you have a complete AI workbench — no server required.

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

Desktop uses a two-column layout:

- **Left sidebar** (fixed 240px): new task, search, the workspace and thread tree, the working directory (file tree + toolbar + usage bar), and settings at the bottom
- **Right column**: the welcome page or the chat page (header + timeline + composer)

File preview and online editing open as overlays without changing the two-column structure. Every user has exactly one agent instance and one fixed working directory.

## 5 Steps to Get Started

### 1. Download and Install

Get the installer for your system from Releases, install and launch.

### 2. First Launch

After launching, a lightweight splash screen appears first while local working directories, configuration, and sessions are prepared. The window appearing does not mean sessions are ready yet; you can start working once the workbench opens. First launches and slower devices may need more preparation time. If the splash screen reports a failure, click retry or reload.

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

### Subagent Activity Indicator

When a task has spawned subagents, the chat area shows that child runs are still working, so you can tell whether the task has really finished.

## Workspace State Messed Up?

If your sessions, subagents, or workspace get into a bad state, use **"Reset Workspace State"** in System Settings.

It clears running states but does NOT delete your skills, knowledge bases, or other long-term assets.

## Local Mode Extras

In Desktop local mode, the chat area also shows:

- 🎤 Microphone button (voice input)
- 📷 Screenshot button (screen capture analysis)

Availability depends on your machine's permissions.

## Need Help? Open the Manual

Settings at the bottom of the left sidebar → Help. You can browse docs without leaving the app.

## Next Steps

- Understand the interface: [Beehive Interface](/docs/en/surfaces/frontend/)
- Understand local mode boundaries: [Desktop Local Mode](/docs/en/ops/desktop-local-mode/)
- Learn about task splitting: [Subagent Control](/docs/en/tools/subagent-control/)
- Running into issues: [Troubleshooting](/docs/en/help/troubleshooting/)
