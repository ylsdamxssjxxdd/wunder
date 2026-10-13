---
title: MCP Endpoint
summary: wunder can both expose itself as an MCP service and act as an MCP client to connect to external services.
updated_at: 2026-10-13
---

MCP has two directions:

- the MCP service wunder exposes to the outside;
- wunder acting as a client to connect to someone else's MCP service.

## Direction: Self-Hosted vs. External

- You want external systems to call wunder's capabilities over MCP → use the self-hosted side.
- You want to bring external capabilities in for the model to use → use the external-service side.

## Expose wunder

The outward entry is a single address, served over Streamable HTTP.

The order in which it comes together:

1. The external system connects to that address following the MCP protocol;
2. wunder negotiates the protocol version automatically; when it interoperates with an older service it settles on a version both sides support, so the initialization step does not have to be written by hand;
3. Call the specific tools.

Two tools are built in for external callers: `excute` (execute) and `doc2md` (document to Markdown). The tool name is literally `excute`; call it by exactly that name.

## Connect an External MCP

Declare the MCP service you want to connect in the configuration — give it a name, an address, and a transport — then switch it on. Once enabled, wunder treats it as a callable service. External services also use Streamable HTTP; if the service is written in Python, it negotiates according to the Python side's protocol version and may not carry every latest capability.

A typical example is the external service often mentioned in the documentation, which usually hosts tools such as database query, database export, and knowledge-base query.

## Key Settings and Common Pitfalls

- Suggested integration order: declare the service in the configuration → confirm in Bridge that the tool list can be fetched → then expose it to the model.
- Bridge has MCP configuration and debugging entries where you can refresh the tool list and call remote tools directly to verify them.
- MCP tools are not a side channel: they are aggregated together with built-in tools, A2A tools, skills, knowledge-base tools, and self-built tools into one tool view. To the model, MCP is just part of the tool catalog.

## Further Reading

- [Tool System](/docs/en/concepts/tools/)
- [Configuration Reference](/docs/en/reference/config/)
