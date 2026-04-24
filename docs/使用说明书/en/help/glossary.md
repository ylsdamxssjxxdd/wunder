---
title: Glossary
summary: Core terms in wunder documentation, explained in plain language.
read_when:
  - You're reading wunder docs for the first time
  - You find terms that look similar but aren't sure of the difference
source_docs:
  - docs/系统介绍.md
---

# Glossary

## User

Your identity. Can be a registered account or a temporary virtual name — the system recognizes both.

## Agent

An AI role that executes tasks. Each agent has its own model, tools, and prompt configuration.

## Thread

A continuous conversation. One agent can have multiple threads, each maintaining its own context independently.

## Swarm

A group of agents collaborating on a task. A swarm has one queen bee that assigns tasks and multiple worker bees that execute them.

## Queen Bee

The role in a swarm responsible for coordinating and assigning tasks.

## Worker Bee

The role in a swarm responsible for executing specific tasks.

## User Turn

Every message you send counts as one user turn.

## Model Turn

Every action the model takes (thinking, calling a tool, replying) counts as one model turn. One user turn may contain multiple model turns.

## Workspace

Your persistent file space. Files placed here are not automatically cleaned up.

## Container

An isolated partition within a workspace. Container 0 is your personal space; containers 1–10 are agent execution spaces.

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
- [Workspaces & Containers](/docs/en/concepts/workspaces/)
- [Stream Events Reference](/docs/en/reference/stream-events/)
