---
title: API Index
summary: This is an index page for Wunder's most commonly used interface groups, used for quickly locating endpoints rather than replacing the full API documentation.
read_when:
  - You are looking for which category an interface roughly belongs to
  - You don't want to browse through the complete `docs/API文档.md` directly
source_docs:
  - docs/API文档.md
  - docs/设计文档/01-系统总体设计.md
---

# API Index

This is not a complete API manual, but an index page for Wunder's main interface groups.

## Core Execution Endpoints

- `POST /wunder`
- `POST /wunder/system_prompt`
- `GET /wunder/tools`

Recommended reading:

- [wunder API](/docs/en/integration/wunder-api/)

## Chat Session Domain

- `GET/POST /wunder/chat/sessions`
- `POST /wunder/chat/attachments/convert`
- `POST /wunder/chat/attachments/media/process`
- `POST /wunder/chat/sessions/{session_id}/messages`
- `GET /wunder/chat/sessions/{session_id}/resume`
- `POST /wunder/chat/sessions/{session_id}/cancel`
- `GET /wunder/chat/ws`

Recommended reading:

- [Chat Sessions](/docs/en/integration/chat-sessions/)
- [Temp Directory and Document Conversion](/docs/en/integration/temp-dir/)
- [Chat WebSocket](/docs/en/integration/chat-ws/)
- [Sessions and Rounds](/docs/en/concepts/sessions-and-rounds/)

## A2A

- `POST /a2a`
- `GET /.well-known/agent-card.json`
- `GET /a2a/agentCard`
- `GET /a2a/extendedAgentCard`

Recommended reading:

- [A2A Interface](/docs/en/integration/a2a/)

## MCP

- `POST /wunder/mcp`
- `GET/POST /wunder/admin/mcp`
- `POST /wunder/admin/mcp/tools`
- `POST /wunder/admin/mcp/tools/call`

Recommended reading:

- [MCP Endpoint](/docs/en/integration/mcp-endpoint/)

## User World

- `GET /wunder/user_world/contacts`
- `GET /wunder/user_world/groups`
- `GET /wunder/user_world/conversations`
- `GET /wunder/user_world/ws`

## Admin Interfaces

Most admin interfaces are under:

- `/wunder/admin/*`

They cover:

- Models
- Tools
- Users and Organizations
- Preset Agents
- Benchmarks
- Channel Governance

## When to Return to Full API Documentation

You should go back to the full documentation when you need:

- Field-level request bodies
- Response structures
- Error codes
- Authentication details
- Backward compatibility fields

## Further Reading

- [wunder API](/docs/en/integration/wunder-api/)
- [Configuration](/docs/en/reference/config/)
- [Troubleshooting](/docs/en/help/troubleshooting/)