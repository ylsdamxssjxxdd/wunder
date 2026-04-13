---
title: Desktop Local Mode
summary: Desktop local mode is not a scaled-down server, but a local-first, persistent-directory-first runtime form.
read_when:
  - You primarily use wunder-desktop
  - You need to determine what local mode versus remote server mode are each suitable for
source_docs:
  - docs/API文档.md
  - src/api/desktop.rs
  - frontend/src/components/messenger/DesktopRuntimeSettingsPanel.vue
updated_at: 2026-04-11
---

# Desktop Local Mode

Desktop local mode is the Wunder runtime form closest to real personal use today.

## First, What NOT to Think of It As

Don't think of it as:

- A mini server
- A server that just lacks the admin backend
- A temporary trial-play mode

Its more accurate positioning is:

- Local-first runtime form
- Desktop workstation
- Complete standalone local working form

## Core Characteristics of Local Mode

### Working Directory is Persistent

In local mode, the working directory is not a temporary sandbox.

- `WUNDER_WORK/` is treated as a persistent directory
- No 24-hour automatic cleanup

### Container Semantics Are Still Preserved

Although it's local mode, it still distinguishes:

- User private containers
- Agent runtime containers

These containers just map to real local directories.

### Prioritizes Bundled Runtime

Desktop local mode prioritizes using the Python runtime bundled with the installer, reducing initial configuration cost.

## Why Local Mode is Suitable as the First Entry Point

Because it covers the complete main pipeline in one go:

- User-side interface
- Conversation execution
- Tool invocation
- Workspace files
- Agent settings
- Swarm collaboration

## What's Special About File Capabilities in Local Mode

A current key convention is:

- In local mode, built-in file tools can access local absolute paths

This is more flexible than pure workspace restrictions, but also means you need to be clearer about local directory boundaries.

## What is One-Click Reset Working State

Local mode now offers `One-Click Reset Working State`.

It's suitable for these scenarios:

- Conversation stuck in running state
- Swarm task state is clearly inconsistent
- Half-finished runtime files left in workspace
- You want the default agent and all custom agents to return to clean main threads

After execution:

- Aborts currently running conversations for the user
- Clears queued tasks
- Terminates current swarm tasks
- Rebuilds main threads for the default agent and all user agents
- Cleans corresponding working state directories

## What Gets Preserved After Reset

This reset targets "working state," not "long-term assets."

Typical content that will be preserved includes:

- `global`
- `skills`
- `knowledge`
- User long-term configurations

So it's more like "cleaning up the runtime scene," not "factory reset."

## When to Switch to Server

If you're just using it personally, local mode is usually sufficient.

When you start needing these capabilities, switch to browser-accessed server form:

- Multi-user governance
- Admin backend
- Organization/company control
- Unified channel and service integration

## Common Pitfalls in Local Mode

- Treating local mode as stateless runtime
- Forgetting that local directories still distinguish containers
- Thinking prompt modifications will write back to the current old thread
- Thinking resetting working state will also delete long-term skills or knowledge

## Further Reading

- [Desktop Getting Started](/docs/en/start/desktop/)
- [User-Side Frontend](/docs/en/surfaces/frontend/)
- [Workspaces and Containers](/docs/en/concepts/workspaces/)