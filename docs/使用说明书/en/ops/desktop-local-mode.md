---
title: Desktop Local Mode
summary: Desktop local mode is a local-first, persistent-directory-first runtime form.
updated_at: 2026-10-13
---

Desktop local mode is a local-first, directory-persistent runtime form. It is not a scaled-down server, and it is not a server that happens to be missing the admin console.

## Local Mode Characteristics

### The Working Directory Is Persistent

In local mode the working directory is not a temporary sandbox. It is treated as a persistent directory and does not go through 24-hour automatic cleanup.

### The Working Directory Is a Real Local Directory

In the local form the working directory sits directly on a persistent directory on the user's own machine:

- Its contents persist and do not go through periodic cleanup
- The agent reads and writes that directory directly, with no round trips between a "sandbox directory" and the real one
- The only difference from the cloud form is location: in the cloud it is a single per-user directory under the server data root, while locally it is a persistent directory on the machine
- Both forms follow "one agent instance and one working directory per user", with no container layering

### Uses the Bundled Runtime First

Desktop local mode prefers the Python runtime bundled with the installer.

## Local Mode Coverage

The complete main pipeline for personal use — the user-side interface, conversation execution, tool calls, workspace files, agent settings, and subagents — is all covered in local mode, so daily use does not require switching to another form.

## File Capabilities in Local Mode

In local mode, the built-in file tools can access local absolute paths. This is more flexible than pure workspace restrictions, and it also means users need to be clearer about the boundaries of local directories.

## One-Click Reset of Working State

Local mode provides a one-click reset of working state, which fits these situations:

- A conversation is stuck in the running state
- Subagent run states are clearly inconsistent
- Half-finished runtime files are left in the workspace
- You want the agent to return to clean task threads

When it runs, it:

- Aborts the conversations the current user still has running
- Clears queued tasks
- Rebuilds task threads for the agent
- Cleans up the corresponding working-state directories

## What Is Preserved After a Reset

A reset targets working state and leaves long-term assets untouched. The typical content it preserves includes global configuration, skills, the knowledge base, and user long-term configuration.

What it does is clean up the runtime scene, rather than restore factory settings.

## When to Switch to a Hull (Server)

If you only use it personally, local mode is usually enough. Once you start needing multi-user governance, the admin console, unit or organization control, and unified channel and service integration, switch to Beehive (web, provided with the Hull) and access it from a browser.

## Boundaries of Local Mode

- Local mode is stateful: the working directory persists and is not run statelessly.
- Local directories are no longer split into a "personal container / agent container": one user corresponds to one agent instance and one working directory, with no container layering.
- Editing a prompt only affects later task threads and does not write back to an old thread that is still running.
- Resetting working state only cleans up the current working state and does not delete long-term skills or knowledge.

## Further Reading

- [Desktop Getting Started](/docs/en/start/desktop/)
- [User-Side Frontend](/docs/en/surfaces/frontend/)
- [Workspaces and Working Directory](/docs/en/concepts/workspaces/)
