---
title: Architecture
summary: wunder uses one unified orchestration kernel. Hive is how users reach that kernel; the deployment form decides where the kernel runs.
read_when:
  - You want to understand wunder as a whole system
  - You need to see how APIs, orchestration, tools, frontends, and storage cooperate
source_docs:
  - docs/设计文档/01-系统总体设计.md
---

# Architecture

wunder has a clear architectural goal: use one unified kernel to support multiple capability sources, reached by users through Hive.

## Hive and deployment forms

How Hive reaches users depends on the deployment form. All three deployment forms run the same kernel behind the scenes; the differences are only in where it runs and what it governs:

### Server (service-side)

**Who it's for**: teams, organizations

**Characteristics**:
- Supports multiple users simultaneously
- Centralized management of users, permissions, and resources
- Can integrate external channels
- Suited for production deployments
- Members access Hive via a web browser

### Desktop (desktop client)

**Who it's for**: individual users

**Characteristics**:
- Local install, runs out of the box
- Can operate local files, windows, and browsers
- Persistent local workspace
- Hive's desktop form

### CLI (command line)

**Who it's for**: developers, automation scenarios

**Characteristics**:
- Terminal-driven, scriptable
- Not Hive — a developer and automation entry
- JSONL output for easy integration

Users spend almost all their time in Hive (Desktop or Server's web). The CLI is a complementary entry for automation and scripting, not Hive.

## Top-level structure

From the repository layout and the current implementation, wunder can be understood in five layers:

1. access layer
2. orchestration layer
3. tools and capability layer
4. storage and workspace layer
5. frontend and desktop-shell layer

## Access layer

The access layer mainly includes:

- the `/wunder` primary interface
- chat WebSocket
- the user-facing frontend
- the admin frontend
- channel entry points
- A2A and MCP entry points

Its role is to unify how the outside world enters the system.

## Orchestration layer

This is the system core, mainly composed of:

- `src/api/`
- `src/orchestrator/`
- `src/services/`
- `src/core/`

It is responsible for:

- parsing requests
- managing sessions and threads
- building model context
- issuing model calls
- handling tool calls
- recording events and state

## Tools and capability layer

wunder's capabilities do not come from one place only. It unifies multiple sources:

- built-in tools
- MCP tools
- Skills
- knowledge-base capabilities
- user tools
- swarm and multi-agent collaboration

This is one of the biggest differences between wunder and a pure chat product.

## Storage and workspace layer

Workspaces and storage are the basis of long-running behavior:

- user workspaces store durable files and artifacts
- session data supports history and replay
- monitoring and events support observability
- long-term memory supports memory injection during thread initialization

## Frontend and desktop-shell layer

wunder currently has two user-visible surfaces:

- Hive (user frontend): `frontend/`, with the desktop shell `desktop/` as its local form — this is where users do daily work
- the admin frontend: `web/` — the governance backend

Hive and the admin frontend share the same underlying capabilities, but serve different interaction goals. The user frontend and admin frontend must remain separate.

## The most important architectural constraints right now

- `server` is the platform core
- `desktop` is Hive's main delivery form for individual users
- `cli` is the developer and automation entry point, not Hive
- Hive and the admin frontend must remain separate
- chat real-time state is WebSocket-only; non-chat streaming endpoints keep their own protocol boundaries

## Diagram

The manual includes a core hierarchy diagram that is useful for establishing system boundaries first:

- [Hierarchy Diagram: wunder -> user -> swarm -> agent -> thread](/docs/assets/manual/08-hierarchy-structure.svg)

## Further reading

- [Sessions and Rounds](/docs/en/concepts/sessions-and-rounds/)
- [Tool System](/docs/en/concepts/tools/)
- [Deployment and Operations](/docs/en/ops/deployment/)
