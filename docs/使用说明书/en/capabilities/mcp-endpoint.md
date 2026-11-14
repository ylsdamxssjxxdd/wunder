---
title: MCP Endpoint
summary: The two directions — the address and two built-in tools when exposing wunder, and the configuration order when connecting an external MCP service.
updated_at: 2026-11-14
---

# MCP Endpoint

wunder can expose itself as an MCP service and act as an MCP client for external services. The two directions have different entry points and settings; tool-level registration, packing and routing are on [MCP](/docs/en/tools/mcp/).

## Exposing wunder

The outward entry is one address over Streamable HTTP.

1. An external system connects with the MCP protocol.
2. wunder negotiates the protocol version automatically, picking a range both sides support when talking to older servers.
3. The system calls a specific tool.

Two tools are built in: `excute` (run an agent task and return the final reply — the name really is spelled this way) and `doc2md` (parse a document and return Markdown). While running a task the endpoint computes the tool set fresh and removes itself, so an endpoint-driven agent cannot call back into it.

## Connecting an external service

1. Declare the service name, address and transport (Streamable HTTP only).
2. Switch it on and confirm in the Admin that the tool list can be fetched.
3. Grant the tools to the agents that need them.

External services usually carry database queries, exports, or knowledge retrieval. Tools register as `server@tool`, or collapse into one tool when the server is marked as packed — see [MCP](/docs/en/tools/mcp/).

## Debug order

If the tool list will not load, check the address and transport first, then whether the service is enabled and whether an allow-list blocks it. If the list loads but the model cannot call the tools, check ability selection and model capability filtering.
