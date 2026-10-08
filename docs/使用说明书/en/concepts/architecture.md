---
title: Architecture
summary: wunder uses one unified orchestration engine. Beehive is how users reach that engine; the deployment form decides where the engine runs.
read_when:
  - You want to understand wunder as a whole system
  - You need to see how APIs, orchestration, tools, frontends, and storage cooperate
source_docs:
  - docs/设计文档/01-系统总体设计.md
---

# Architecture

wunder has a clear architectural goal: use one unified engine to support multiple capability sources, reached by users through Beehive.

## Beehive and deployment forms

How Beehive reaches users depends on the deployment form. All three deployment forms run the same engine behind the scenes; the differences are only in where it runs and what it governs:

### Hull (server)

**Who it's for**: teams, organizations

**Characteristics**:
- Supports multiple users simultaneously
- Centralized management of users, permissions, and resources
- Can integrate external channels
- Suited for production deployments
- Members access Beehive via a web browser

### Honeycomb (desktop)

**Who it's for**: individual users

**Characteristics**:
- Local install, runs out of the box
- Can operate local files, windows, and browsers
- Persistent local workspace
- Beehive's desktop form

### Helm (cli)

**Who it's for**: developers, automation scenarios

**Characteristics**:
- Terminal-driven, scriptable
- Not Beehive — a developer and automation entry
- JSONL output for easy integration

Users spend almost all their time in Beehive (Honeycomb or Hull's web). Helm is a complementary entry for automation and scripting, not Beehive.

## Top-level structure

From the repository layout and the current implementation, wunder can be understood in five layers:

1. access layer
2. orchestration layer
3. tools and capability layer
4. storage and workspace layer
5. frontend layer

## Access layer

The access layer mainly includes:

- the `/wunder` primary interface
- chat WebSocket
- the user-facing frontend
- the admin console (Bridge)
- channel entry points
- A2A and MCP entry points

Its role is to unify how the outside world enters the system.

## Orchestration layer

This is the Engine, mainly composed of:

- `crates/wunder-runtime/src/api/`
- `crates/wunder-runtime/src/orchestrator/`
- `crates/wunder-runtime/src/services/`
- `crates/wunder-runtime/src/core/`

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
- temporary subagents spawned by the main agent

This is one of the biggest differences between wunder and a pure chat product.

## Storage and workspace layer

Workspaces and storage are the basis of long-running behavior:

- user workspaces store durable files and artifacts
- session data supports history and replay
- monitoring and events support observability
- long-term memory supports memory injection during thread initialization

## Frontend layer

wunder currently has two user-visible surfaces:

- Beehive (user workbench): `frontend/`, delivered as the Honeycomb desktop app or Hull's web form — this is where users do daily work
- Bridge (admin console): `web/` — the governance backend

Beehive and Bridge share the same underlying capabilities, but serve different interaction goals. The user frontend and the admin console must remain separate.

## The most important architectural constraints right now

- `server` (Hull) is the server form for teams and organizations
- Honeycomb (the desktop app) is Beehive's main delivery form for individual users
- `cli` (Helm) is the developer and automation entry point, not Beehive
- Beehive and Bridge must remain separate
- all three runtime forms build on the same Engine; Honeycomb and Helm do not depend on the Hull
- chat real-time state is WebSocket-only; non-chat streaming endpoints keep their own protocol boundaries

## Diagram

The manual includes a core hierarchy diagram that is useful for establishing system boundaries first:

- [Hierarchy Diagram: wunder -> user -> agent -> thread](/docs/assets/manual/08-hierarchy-structure.svg)

## Further reading

- [Sessions and Rounds](/docs/en/concepts/sessions-and-rounds/)
- [Tool System](/docs/en/concepts/tools/)
- [Deployment and Operations](/docs/en/ops/deployment/)
