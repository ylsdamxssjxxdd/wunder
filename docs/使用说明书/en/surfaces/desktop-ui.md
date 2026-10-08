---
title: Desktop Interface
summary: Beehive's desktop form. Local-first, with direct file access and one-click reset.
---

# Desktop Interface

Desktop is Beehive's local form, the main entry for individual users. It's not just a "shell" for the web — it's a local-first complete workbench.

## Desktop Features

### Local-First

- Data stored locally, strong privacy
- No server dependency, works out of the box

### Local File Access

Can directly access the local file system (within security boundaries), more flexible than the web-only version.

### Built-in Runtime

The installer includes a Python runtime, reducing first-time setup cost.

### One-Click Reset

Reset workspace state with one click when things go wrong.

## Interface Structure

Desktop reuses Beehive's two-column layout, adding desktop-specific local capabilities:

- **Left sidebar** (fixed 240px): new task, search, the workspace and thread tree, the working directory (file tree + toolbar + usage bar), and settings at the bottom (including desktop runtime settings and one-click reset)
- **Right column**: the welcome page or the chat page

Both Desktop and the web version follow "one agent instance and one working directory per user", so the sidebar holds only that single workspace and thread tree — no multi-agent list, create, or switch entry, and no way to change the directory.

## Desktop-Specific Features

### Desktop Runtime Settings

Configure the local runtime environment: Python path, local working directory, local tool configuration.

### One-Click Reset Workspace State

**When to use**:
- Session stuck in running state
- Subagent run state inconsistent
- Interface state doesn't match reality

**After reset**:
- Stops all running sessions
- Clears queued tasks
- Rebuilds clean threads

**What's preserved**:
- Skills, knowledge bases, and other long-term assets
- User configuration
- Workspace files
- Session history

So reset is "cleaning workspace state", not "factory reset".

### Local File Capabilities

Desktop allows agents to:
- Read local files
- Write local files
- Execute local commands

Desktop agents get local file and command access that the web version does not have, still within security boundaries.

## When to Use Desktop

**Good for**:
- Personal daily use
- Need to work with local files
- Don't want to depend on a server
- Care about privacy and local data

**If you need**:
- Multi-user collaboration
- Admin backend
- Organization-level deployment

Use the browser to access Server's web interface instead.

## Common Questions

### Where is my data?

- Workspace: Local working directory
- Session history: Local SQLite database
- Configuration: Local config files

### Can local mode access the internet?

Yes. Local mode only means data is stored locally. It doesn't affect internet access for AI models or web pages.

### Can I migrate to another computer?

Yes. Copy the working directory and database files, then configure the paths on the new computer.

## Further Reading

- [Desktop Guide](/docs/en/start/desktop/)
- [Beehive Interface](/docs/en/surfaces/frontend/)
- [Desktop Local Mode](/docs/en/ops/desktop-local-mode/)
