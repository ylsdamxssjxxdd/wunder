---
title: Glossary
summary: Core terms in wunder documentation, explained in plain language.
updated_at: 2026-10-13
---

## Product and Form Names

| Name | Short id | Meaning |
|------|------|------|
| wunder | wunder | The product itself, the umbrella term for the **agent orchestration platform**: hand a goal to the agent, and it breaks the task down, calls tools, and delivers results. |
| Core | core | The foundation: configuration, authentication, storage, and execution policy. |
| Engine | runtime | The execution layer: task orchestration, thread runtime, tools, channels, and gateway. The Honeycomb and the Helm depend on the Engine directly, not on the Hull. |
| Hull | server | The **user and agent management platform**: the server form, aimed at teams and organizations, handling multi-user access, permissions, and channel access, and providing the Beehive. |
| Bridge | bridge | The governance frontend: the system administrator's management interface for system configuration and for user and channel governance. |
| Bridge node | bridge node | A channel bridging node: it carries channel account access and message routing and is managed uniformly by the backend, with no standalone frontend page today. Keep it distinct from the governance frontend "Bridge". |
| Beehive | web | The **cloud AI workbench**: the web form provided with the Hull, which users open in a browser. |
| Honeycomb | desktop | The **local AI workbench**: the desktop form, the default entry point for individual users, running locally with direct access to local files. |
| Helm | cli | The **manual local entry point**: the command-line form, aimed at hands-on operation, developers, and automation scripts. |

Hull, Bridge, Beehive, Honeycomb, and Helm name concrete forms of wunder; Core and Engine name the underlying layers. Keep the two groups separate. The first mention in the body pairs a form's display name with its short id — for example Hull (server), Honeycomb (desktop), Helm (cli).

## User

Your identity marker. It can be a registered account or a temporary virtual name; the system recognizes both.

## Agent

The AI role that executes tasks. An agent has its own model, tools, and prompt configuration. Every user has **exactly one** agent instance, generated from a 1:1 binding to an admin-side preset; there is no user-side entry point to create, delete, switch, or reorder agents, and only the fields a preset marks as customizable can be changed.

## Preset Agent

An agent template maintained by the admin. There can be several presets, but each user binds exactly one of them, which generates that user's single agent instance. A preset update can be pushed to bound users as "sync uncustomized fields / force overwrite".

## Thread

One continuous conversation. One agent can have several threads, and each thread maintains its own context independently.

## Subagent

A temporary work unit the main agent creates during a task and reclaims once the task is done. It is not a separate agent, and not a second agent the user owns.

## Workspace

Your persistent file space. Every user has **exactly one** workspace, which tools use as their root: in the Honeycomb (desktop) it is a persistent directory on your own machine, with direct read and write access to local files; in a Hull (server) deployment it is the single per-user directory under the server data root, reached through the Beehive. A workspace cannot be added, deleted, or replaced.

## Working Directory

The workspace entry point in the UI: below the workspace in the left column of the web app, holding a file tree, a toolbar (upload / new / refresh / multi-select), a usage bar, and a preview overlay. It is not an operating-system file browser.

## User Turn

Every message a user sends is one user turn.

## Model Turn

Every action the model takes (thinking, calling a tool, replying) is one model turn. One user turn may contain several model turns.

## Skill

A capability package for the model. It usually contains documentation, scripts, and resource files that let an agent handle specific types of tasks.

## MCP

Model Context Protocol. A standard interface that lets wunder connect to external tool services.

## A2A

An interoperability protocol between agents. It lets agents from different systems discover and collaborate with each other.

## Channel

An external messaging path, such as Feishu, WeCom, QQ, or XMPP. Through channels, an agent can send and receive external messages.

## Token

The basic unit by which a model processes text. The token count reflects the current conversation's context length and does not equal the actual cost.

## Context Compression

When a conversation gets too long, the system automatically compresses the history so the model can keep working without losing key information.

## Long-term Memory

Knowledge an agent remembers across conversations. You can add it manually, or let the system distill it automatically.

## Further Reading

- [Sessions and Turns](/docs/en/concepts/sessions-and-rounds/)
- [Workspaces and Working Directory](/docs/en/concepts/workspaces/)
- [Stream Events Reference](/docs/en/reference/stream-events/)
