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

Desktop local mode is the wunder runtime form closest to real personal use today.

## First, What NOT to Think of It As

Don't think of it as:

- A mini server
- A server that just lacks the admin backend
- A temporary trial-play mode

Its more accurate positioning is:

- Local-first runtime form
- Desktop workstation
- Complete standalone local working form

## Key Characteristics of Local Mode

### Working Directory is Persistent

In local mode, the working directory is not a temporary sandbox.

- `WUNDER_WORK/` is treated as a persistent directory
- No 24-hour automatic cleanup

### The Working Directory Is a Real Local Directory

In the local form the working directory sits directly on a persistent directory on your machine:

- contents persist and are not subject to periodic cleanup
- the agent reads and writes that directory directly, with no mapping back and forth between a "sandbox directory" and the real one
- the only difference from the cloud form is location: in the cloud it is a single per-user directory under the server data root; locally it is a persistent directory on your machine
- both forms follow "one agent instance and one working directory per user", with no container layering

### Prioritizes Bundled Runtime

Desktop local mode prioritizes using the Python runtime bundled with the installer, reducing initial configuration cost.

## Why Local Mode is Suitable as the First Entry Point

Because it covers the complete main pipeline in one go:

- User-side interface
- Conversation execution
- Tool invocation
- Workspace files
- Agent settings
- Subagents

## What's Special About File Capabilities in Local Mode

A current key convention is:

- In local mode, built-in file tools can access local absolute paths

This is more flexible than pure workspace restrictions, but also means you need to be clearer about local directory boundaries.

## What is One-Click Reset Working State

Local mode now offers `One-Click Reset Working State`.

It's suitable for these scenarios:

- Conversation stuck in running state
- Subagent run state is clearly inconsistent
- Half-finished runtime files left in workspace
- You want the agent to return to clean task threads

After execution:

- Aborts currently running conversations for the user
- Clears queued tasks
- Rebuilds task threads for the agent
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

When you start needing these capabilities, switch to the Hull (server) form accessed from a browser:

- Multi-user governance
- Admin backend
- Organization/company control
- Unified channel and service integration

## Common Pitfalls in Local Mode

- Treating local mode as stateless runtime
- Expecting local directories to be split again into "personal container / agent container"
- Thinking prompt modifications will write back to the current old thread
- Thinking resetting working state will also delete long-term skills or knowledge

## Further Reading

- [Desktop Getting Started](/docs/en/start/desktop/)
- [User-Side Frontend](/docs/en/surfaces/frontend/)
- [Workspaces and Working Directory](/docs/en/concepts/workspaces/)