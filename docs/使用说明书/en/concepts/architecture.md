---
title: Architecture
summary: wunder uses one unified orchestration kernel to support three runtime forms: server, cli, and desktop.
read_when:
  - You want to understand wunder as a whole system
  - You need to see how APIs, orchestration, tools, frontends, and storage cooperate
source_docs:
  - docs/设计文档/01-系统总体设计.md
---

# Architecture

wunder has a clear architectural goal: use one unified kernel to support multiple runtime forms and multiple capability sources.

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

wunder currently has at least three user-visible surfaces:

- user frontend: `frontend/`
- admin frontend: `web/`
- desktop shell: `desktop/`

They share the same underlying capabilities, but each serves a different interaction goal.

## The most important architectural constraints right now

- `server` is the platform core
- `desktop` is the main delivery form
- `cli` is the developer and automation entry point
- the user frontend and admin frontend must remain separate
- chat real-time state is WebSocket-only; non-chat streaming endpoints keep their own protocol boundaries

## Diagram

The manual includes a core hierarchy diagram that is useful for establishing system boundaries first:

- [Hierarchy Diagram: wunder -> user -> swarm -> agent -> thread](/docs/assets/manual/08-hierarchy-structure.svg)

## Further reading

- [Sessions and Rounds](/docs/en/concepts/sessions-and-rounds/)
- [Tool System](/docs/en/concepts/tools/)
- [Deployment and Operations](/docs/en/ops/deployment/)
