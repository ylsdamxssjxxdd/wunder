---
title: Glossary
summary: Core terms in wunder documentation, explained in plain language.
read_when:
  - You're reading wunder docs for the first time
  - You find terms that look similar but aren't sure of the difference
source_docs:
  - docs/设计文档/01-系统总体设计.md
---

# Glossary

## User

Your identity. Can be a registered account or a temporary virtual name — the system recognizes both.

## Agent

An AI role that executes tasks. An agent has its own model, tools, and prompt configuration. Every user has **exactly one** agent instance, generated from a 1:1 admin-side preset binding; there is no user-side entry point to create, delete, switch, or sort agents, and only fields the preset declares customizable can be changed.

## Preset Agent

An agent template maintained by the admin. There can be many presets, but each user binds exactly one of them, which generates that user's single agent instance. Preset updates can be pushed to bound users as "sync uncustomized fields" or "force overwrite".

## Thread

A continuous conversation. One agent can have multiple threads, each maintaining its own context independently.

## Subagent

A temporary work unit the main agent creates for a task and reclaims when the task is done. It is not a separate agent, and not a second agent owned by the user.

## User Turn

Every message you send counts as one user turn.

## Model Turn

Every action the model takes (thinking, calling a tool, replying) counts as one model turn. One user turn may contain multiple model turns.

## Workspace

Your persistent file space. Every user has **exactly one** workspace, used as the root for file tools: in the Honeycomb (desktop) it is a persistent local directory with direct access to local files; in Hull (server) deployments it is a single per-user directory under the server data root, accessed through the web UI. A workspace cannot be added, deleted, or replaced.

## Working Directory

The workspace entry point in the UI: below the workspace tree in the left sidebar of the web app, with a file tree, a toolbar (upload / new / refresh / multi-select), a usage bar, and preview overlays. It is not an operating-system file browser.

## Skill

A capability package for the model. Typically includes documentation, scripts, and resource files that enable an agent to handle specific types of tasks.

## MCP

Model Context Protocol. A standard interface that lets wunder connect to external tool services.

## A2A

Agent-to-Agent protocol. Enables agents from different systems to discover and collaborate with each other.

## Channel

An external messaging pathway, such as Feishu, WeCom, QQ, XMPP, etc. Through channels, agents can send and receive external messages.

## Token

The basic unit of measurement for model text processing. Token count reflects the current conversation's context length, not actual cost.

## Context Compression

When a conversation gets too long, the system automatically compresses historical content so the model can continue working without losing key information.

## Long-term Memory

Knowledge that an agent remembers across conversations. Can be added manually or automatically extracted by the system.

## Further Reading

- [Sessions & Turns](/docs/en/concepts/sessions-and-rounds/)
- [Workspaces and Working Directory](/docs/en/concepts/workspaces/)
- [Stream Events Reference](/docs/en/reference/stream-events/)
