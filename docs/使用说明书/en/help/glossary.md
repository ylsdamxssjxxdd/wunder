---
title: Glossary
summary: This page is for quickly standardizing core terminology in Wunder documentation, avoiding mixing user, session, thread, container, and agent into a single concept.
read_when:
  - You're reading Wunder documentation systematically for the first time
  - You notice many terms look similar but don't know the boundaries
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
  - docs/API文档.md
---

# Glossary

## `user_id`

User identifier in Wunder requests.

It can be a registered user, or just a virtual isolation identifier.

## Registered User

Refers to an account that actually exists in the user management system.

This is different from any arbitrary `user_id` passed in `/wunder` calls.

## `agent_id`

Agent application identifier.

It typically determines:

- Agent configuration
- Additional prompts
- Tool mounting
- Container routing

## `session_id`

Identifier for a specific conversation session.

It's the most important context ID for conversation recovery and continuing to send messages.

## Thread

When Wunder documentation says "thread", it's usually not an OS thread, but a session-level execution unit.

It binds:

- Frozen system prompt
- Historical messages
- Current runtime state

## User Turn / Model Turn

Each message a user sends counts as 1 user turn.

Each action a model executes counts as 1 model turn. Actions include:

- Model call
- Tool call
- Final response

## `container_id`

Workspace container number.

Current convention:

- `0`: User private container
- `1~10`: Agent execution containers

## Workspace

Wunder's persistent file space.

It's isolated by `user_id + container_id`, not simply "current directory".

## `skill`

Model-oriented skill package.

Usually contains:

- `SKILL.md`
- Scripts
- Resource files

## `skill_call`

Built-in skill invocation tool.

The model can use it to directly read skill content and directory structure.

## MCP

Model Context Protocol integration surface.

In Wunder, it can be either a service Wunder exposes, or an external service Wunder connects to.

## A2A

Standard inter-agent interoperability protocol integration surface.

It's more "system-to-system" oriented, not ordinary business interfaces.

## `channel`

External message channel.

Examples include Feishu, WeCom, QQBot, XMPP, etc.

## `outbox`

Channel outbound buffer and retry layer.

It transforms channel sending from synchronous operations to resumable async pipelines.

## `turn_terminal`

Terminal event of a turn execution.

When determining if a turn has ended, check this first.

## `thread_status`

Thread current runtime state event.

When determining if a thread is running, waiting for approval, or idle, check this first.

## `approval_resolved`

Approval closed-loop event.

Indicates that a pending approval request has reached a terminal state.

## Further Reading

- [Sessions and Rounds](/docs/en/concepts/sessions-and-rounds/)
- [Workspaces and Containers](/docs/en/concepts/workspaces/)
- [Stream Events Reference](/docs/en/reference/stream-events/)