---
title: MCP
summary: The two MCP directions — `server@tool` registration and routing for external servers, packing mode and the streamable-http limit, plus the outgoing `/wunder/mcp` endpoint and common failures.
updated_at: 2026-11-14
---

# MCP

MCP is the common protocol between wunder and external tool services. It runs in two directions with separate entry points and configuration: pulling an external service in for the model, and exposing wunder to an outside system.

## Pulling in an external service

Declare a name, an address, and a transport in configuration, switch it on, and its tools appear in the tool catalog.

- **One transport only**: streamable-http. stdio is not supported on the wunder side, and older transport values are rejected when listing or calling tools.
- **Registered as `server@tool`**: every allowed tool appears under that name, and any name containing `@` is treated as MCP.
- **Packing mode**: mark a server as packed and it folds into **one** tool with three actions — list available tools, read one tool's description, call one tool. Dozens of details stay out of the context until asked for.
- **List cache**: valid for 30 seconds, at most 128 entries.
- **Object arguments only**: a non-object argument is treated as no arguments.
- **Result shape**: a direct call returns the MCP service's own result; a call routed through a user-tool alias gets the shared shell wrapped around it.

The allow list is checked on every call, packed or not: a tool that is not allowed never appears and cannot be called.

## Exposing wunder

wunder is also an MCP server: the Server and Desktop mount an endpoint at `/wunder/mcp` over Streamable HTTP, and wunder negotiates the protocol version itself.

| Tool | What it does |
| --- | --- |
| `excute` | Runs one agent task and returns the final reply (the name really is spelled this way) |
| `doc2md` | Parses a document and returns Markdown |

When the endpoint runs a task it computes the tool set for that agent and removes itself from it, so an endpoint-driven agent cannot call the endpoint back. Address and verification details are on [MCP Endpoint](/docs/en/capabilities/mcp-endpoint/).

## Common failures

| Symptom | Check first |
| --- | --- |
| Server configured, no tools | Whether it is enabled, whether the allow list blocks it, whether the transport value is stale |
| "Old transport type no longer supported" | Switch to streamable-http |
| "Unknown built-in tool" | The name is not a canonical name or alias, has no `@`, is not a skill, and is not a knowledge base |
| A user tool "took over" a built-in name | Alias routing wins over built-in dispatch; this is expected |

## Boundaries

- MCP tools get no bypass: they join built-in tools, skills, and knowledge bases in one tool view, and pass the ability selection set and model availability filters like everything else.
- Dynamically registered names share one table with built-ins — avoid built-in canonical names and aliases when naming servers and tools.
- Knowledge bases register by name too; see [Tools Overview](/docs/en/tools/). Skills live on [Skills](/docs/en/tools/skills/).
