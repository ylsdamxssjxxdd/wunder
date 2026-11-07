---
title: Admin Interface
summary: The governance and debugging console built into the Server, visible to administrators only. Top-level navigation has 4 groups and 19 items: System 4, Agents 6, Debug 6, Docs 2; plus notes on panels never implemented.
read_when:
  - You administer the Server as an administrator through the Admin
  - You want to know what the Admin does not have (memory, interconnection, goal editing)
source_docs:
  - web/index.html
  - web/app.js
  - web/modules/log.js
  - crates/wunder-server/src/main.rs
updated_at: 2026-10-16
---

# Admin Interface

The Admin is the governance and debugging console built into the **Server (the user and agent management platform)**. Regular users never see it after signing in; only administrators get in, and daily conversation happens in the Cloud or the Desktop.

It starts along with the Server, with no separate build step, and shares the Server's port (18000 by default). The Cloud and it are **two separate interfaces**: the Cloud faces users, the Admin faces admins.

## Top-level navigation

The Admin has 4 groups and 19 items.

**System (4)**

- Internal status: process health, locks and queues, runtime metrics
- System settings: hot-update configuration while running
- Organization management: the organization tree, up to 4 levels deep
- User management: accounts, quotas, and online status (split by Cloud / Desktop / CLI)

**Agents (6)**

- Model configuration: LLM / TTS / image / video models
- Thread management: a cross-user thread list with status and goal state (**read-only display**, with no goal editor)
- Preset agents: agent templates that can declare which fields users may customize; user-level scheduled tasks and channels exist here as templates rather than as separate navigation items
- Companion management
- Tool management: groups built-in tools, MCP, knowledge bases, skills, and tool visibility together
- System prompts: prompt templates

**Debug (6)**

- Throughput test: preset inputs from 1024 bytes to 1 MiB and outputs from 1024 to 8192, with a maximum concurrency of 1024
- Performance test: uses fixed test users and a separate data root, with a 5-second command timeout
- Capability evaluation: runs capability benchmarks and scores them
- Multimodal debugging: direct tests of image / video / transcription / speech synthesis
- Debug panel: thread list + thread details; the **log area on the right is now a thread-trajectory view**, with the old log output kept only as a fallback
- LSP: language service

**Docs (2)**

- Manual: the whole manual embedded in the console
- API: the API documentation entry

The throughput test and the performance test are the Admin's operations panels; the rendering-oriented thread stress test belongs to the user side, where the three interfaces default to 1000×1000.

## Panels never implemented

- **A user-level scheduled-task panel**: there is no separate entry; scheduling exists only as a template under "Preset agents"
- **User-level channel management**: the same — channels live only in preset templates
- **A long-term memory panel**: not implemented
- **An interconnection governance panel**: fleet / channel / command / audit / policy governance for device interconnection is backend-API-only for now, with no interface; it is not true that the Admin can govern interconnection
- **Goal editing**: thread management is read-only, with no goal editor; goal entry points are on the user side — the Cloud timeline, the Desktop input area, and the CLI's `/goal` command

Status may still show interconnection and memory entries whose panels are empty; those are leftover dead entries, since the panels were never implemented.

## Operational constraints

- Key actions all have clear status, error feedback, and confirmation boundaries; a front-end confirmation does not replace backend permission checks
- Tables, logs, and monitoring panels use pagination / filtering / limits, so they never dump a large amount of data at once
- The interface is dense and made for scanning

## Access and authentication

- Address: `http://<host>:18000/` (the default port, changeable in configuration)
- Authentication: admins sign in with an API key or login credentials; an organization head can manage only their own organization unit and its user accounts
- Change the default password right after the first sign-in
