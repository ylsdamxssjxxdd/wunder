---
title: Glossary
summary: Form and layer names in wunder, plus definitions for threads, turns, workspaces, and other core terms.
updated_at: 2026-11-14
---

## Product and Form Names

| Name | Short id | Meaning |
|------|------|------|
| wunder | wunder | The product itself, a **general-purpose AI operating system**: wunder = model + harness + runtime — everything except the model. Hand a goal to the agent, and it breaks the task down, calls tools, and delivers results. |
| Core | core | The foundation: configuration, authentication, storage, and execution policy. |
| Engine | runtime | The execution layer: task orchestration, thread runtime, tools, channels, and gateway. The Desktop, Android, and the CLI depend on the Engine directly, not on the Server. |
| Server | server | The **user and agent management platform**: the server form, aimed at teams and organizations, handling multi-user access, permissions, and channel access, and providing the Cloud. |
| Admin | admin (`web/`) | The governance frontend: the system administrator's management interface for system configuration and for user and channel governance. Do not confuse it with the bridge of a channel bridge node. |
| Channel bridge node | bridge node | A channel bridging node: it carries channel account access and message routing and is managed uniformly by the backend, with no standalone frontend page today. Keep it distinct from the governance frontend "Admin". |
| Cloud | web | The **cloud AI workbench**: the web form, provided with the Server and opened by users in a browser. English name wunder Cloud, formerly Hive. |
| Desktop | desktop | The **local AI workbench**: the desktop form, the default entry point for individual users, running locally with direct access to local files. English name wunder Desktop, formerly Pupa. |
| Android | android | **The Desktop's Android form, a local AI workbench on a phone**: install the APK; a single native Activity shell reuses the Desktop UI components and calls the engine in-process with a local SQLite. |
| CLI | cli | The **manual local entry point**: the command-line form, aimed at hands-on operation, developers, and automation scripts. |

Server, Admin, Cloud, Desktop, Android, and CLI name concrete forms of wunder; Core and Engine name the underlying layers. Keep the two groups separate. The first mention in the body may pair a form's display name with its short id — for example Cloud (web).

## User

Your identity marker. It can be a registered account or a temporary virtual name; the system recognizes both.

## Agent

The AI role that executes tasks. An agent has its own model, tools, and prompt configuration. Every user has **exactly one** agent instance, generated from a 1:1 binding to an admin-side preset; there is no user-side entry point to create, delete, switch, or reorder agents, and only the fields a preset marks as customizable can be changed.

## Preset Agent

An agent template maintained by the admin. There can be several presets, but each user binds exactly one of them, which generates that user's single agent instance. Applying a preset updates its system-managed configuration only for bound users and preserves user-created MCP, skills, and knowledge-base selection states.

## Thread

One continuous conversation. One agent can have several threads, and each thread maintains its own context independently.

## Subagent

A temporary work unit the main agent creates during a task and reclaims once the task is done. It is not a separate agent, and not a second agent the user owns.

## Workspace

Your persistent file space; tool execution is bounded by the path fence. In a Server deployment every user has exactly one cloud workspace, a per-user directory under the server data root reached through the Cloud. The Desktop and Android can register several workspaces (local directories, the cloud workspace, and remote device workspaces reached through device interlink) and switch by `workspace_id`.

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

An interoperability protocol between agents. **Not implemented today**: the Rust side has no A2A routes; only desktop icon mappings and an nginx reverse-proxy remnant remain, so do not treat it as a current capability.

## Channel

An external messaging path, such as Feishu, WeCom, QQ, or XMPP. Through channels, an agent can send and receive external messages.

## Token

The basic unit by which a model processes text. The token count reflects the current conversation's context length and does not equal the actual cost.

## Context Compression

When a conversation gets too long, the system automatically compresses the history so the model can keep working without losing key information. The current main chain is full compaction (a summary model produces the replacement history); standalone micro-compaction has been removed, and old `type=microcompaction` markers are replay-compatible only.

## Long-term Memory

Knowledge an agent remembers across conversations. It is injected once as an L0 index at thread creation and then frozen; the body text is pulled on demand through the memory-management tool. You can add it manually, or let the system distill it at the end of a turn. Semantic recall is currently disabled, and `recall` is only an alias of `search`.
