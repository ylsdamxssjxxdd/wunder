---
title: MCP Endpoint
summary: wunder supports both a self-hosted MCP service at `/wunder/mcp` and external MCP service integration.
read_when:
  - You want to expose wunder as an MCP service
  - You need to understand the relationship between wunder's internal MCP and extra_mcp
source_docs:
  - docs/API文档.md
  - docs/设计文档/01-系统总体设计.md
  - src/services/mcp.rs
  - config/wunder-example.yaml
---

# MCP Endpoint

MCP is not a secondary capability in wunder -- it is a first-class integration surface.

You need to distinguish two things up front:

1. wunder exposes its own MCP service at `/wunder/mcp`
2. wunder can also act as an MCP client to connect to external services, such as `extra_mcp`

## Self-Hosted MCP Endpoint

- `POST /wunder/mcp`
- Transport: Streamable HTTP

The Rust backend currently exposes two built-in tools:

- `excute`
- `doc2md`

Note that the tool name is literally `excute` -- the documentation reflects the current code as-is. Do not rewrite it as `execute`.

## When to Use the Self-Hosted MCP

Suitable for these scenarios:

- Letting external systems invoke wunder capabilities via MCP
- Integrating wunder into another MCP orchestration framework
- Exposing internal task execution and document parsing capabilities under a unified protocol

## Configuration Example

```yaml
mcp:
  servers:
    - name: wunder
      endpoint: http://127.0.0.1:8000/wunder/mcp
      enabled: false
      transport: streamable-http
```

Once enabled, wunder treats this MCP service as a callable MCP server.

## External MCP vs extra_mcp

The repository also retains a typical external MCP service:

- `extra_mcp`

It is typically used to host:

- `db_query`
- `db_export`
- `kb_query`

In other words:

- `/wunder/mcp` is for "what wunder exposes outward"
- `extra_mcp` is for "external capabilities wunder brings in"

## How the Admin Panel Handles MCP

The admin panel provides a set of MCP configuration and debugging tools for:

- Configuring `mcp.servers`
- Refreshing the tool list
- Debugging remote tool invocations

The typical integration order is therefore:

1. Declare the MCP server in configuration
2. Verify in the admin panel that tool specs can be fetched
3. Expose the tools to the model via the agent or tool catalog

## Relationship Between MCP and the Tool Catalog

wunder does not treat MCP as a side channel.

It aggregates MCP tools together with these capabilities into a unified tool view:

- Built-in tools
- A2A tools
- Skills
- Knowledge base tools
- User-created tools

So from the model's perspective, MCP tools are ultimately part of the tool catalog.

## Further Reading

- [Tool System](/docs/en/concepts/tools/)
- [A2A Interface](/docs/en/integration/a2a/)
- [Configuration Reference](/docs/en/reference/config/)
