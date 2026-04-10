---
title: Desktop Guide
summary: If your first goal is simply to start using wunder, begin with desktop. It is the default entry point for individual and local-first use.
read_when:
  - You want to start using wunder immediately
  - You care more about the desktop workstation than deploying a full server stack
source_docs:
  - docs/API文档.md
  - frontend/src/components/messenger/DesktopRuntimeSettingsPanel.vue
  - frontend/src/components/messenger/MessengerSettingsPanel.vue
updated_at: 2026-04-10
---

# Desktop Guide

If you want to start using wunder right away, do not deploy server first. Start with `wunder-desktop`.

It is built for individual users and local-first scenarios, and opens directly into a full agent workstation.

## When to choose Desktop

| Scenario | Choose Desktop |
|------|-----------|
| You want to start now without deploying services first | ✅ |
| You need local files, windows, and browser capabilities | ✅ |
| You prefer a graphical interface | ✅ |
| You may connect to a remote backend later, but need something running now | ✅ |
| You need multi-user governance and an admin backend | ❌ Choose Server |

## What Desktop is

Desktop is not a simplified Server. It is an independent local-first runtime form:

```text
wunder-desktop
  ├─ User frontend (chat, profile, swarm, settings)
  ├─ Local bridge (screenshots, recording, desktop environment capabilities)
  └─ Rust core engine (orchestration, tools, workspace)
```

## What you see after launch

Desktop still uses the unified three-column layout:

- left: navigation
- center: conversations, agents, swarms, and resources
- right: chat area, workflows, profile pages, and settings

It is not only a chat window. It also includes:

- conversations between users and agents
- workspace and file management
- swarm collaboration and multi-agent workflows
- system settings and local runtime preferences
- your profile and account settings

## Five steps to get started

### 1. Download and install

- Download the installer for your operating system from Releases

### 2. First launch

On first launch, Desktop automatically:

- creates the local working directory `WUNDER_WORK/`
- creates the configuration directory `WUNDER_TEMPD/`
- prepares built-in skills and runtime resources

### 3. Configure a model

Go to `System Settings` and configure:

- API key
- endpoint
- model name

Before saving, it is best to click `Test Connection` once.

### 4. Set up your account details

If this is your own account, it is worth handling these items early:

- on the login page, confirm whether you need to register or reset the password
- go to `My Profile -> Edit Profile` to update the username or email
- if this will be a long-lived account, set a new login password as well

Password reset from the login page only requires:

- username
- email
- new password

Changing the password after login still requires the current password for verification.

### 5. Start the first conversation

Go back to chat and enter:

```text
Help me list the files in the current working directory
```

You will see:

- the model starts executing
- the tool workflow appears step by step
- the final reply lands in the chat area

## Two interaction constraints you will hit quickly

### You cannot create a new thread while one is still running

While the current agent is still running, the `New Thread` button is disabled.

This avoids state conflicts between the active main thread and a newly created thread. The correct flow is:

- wait for the run to finish
- or stop the current session first, then create a new thread

### Active swarms keep a visible highlight

If a swarm is still active:

- its item in the center column keeps a pulsing highlight
- the queen, workers, and subagent nodes on the canvas keep their workflow traces

## Recovery entry in System Settings

If conversations, swarms, or the workspace end up in a bad runtime state, use `One-click Reset Working State` in `System Settings`.

It will:

- stop running sessions and tasks
- terminate active swarm runs
- rebuild the main thread for the default agent and user agents
- clean working-state directories

It will not delete long-lived user assets such as:

- `global`
- `skills`
- `knowledge`

## Extra input capabilities in local mode

When Desktop is running in local mode, the chat area usually also shows:

- a microphone button
- a screenshot button

Whether these buttons are available depends on local machine permissions and desktop bridge capabilities.

## When in doubt, open the manual

Desktop already includes a built-in manual entry:

- go to `More` in the left column
- select `Help Manual` in the center column
- the right column opens the docs site directly

## Next

- To understand the UI layout, see [Frontend Surface](/docs/en/surfaces/frontend/)
- To understand local-mode boundaries, see [Desktop Local Mode](/docs/en/ops/desktop-local-mode/)
- To understand swarm collaboration, see [Swarm Collaboration](/docs/en/concepts/swarm/)
- If something goes wrong, see [Troubleshooting](/docs/en/help/troubleshooting/)
