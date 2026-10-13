---
title: A2A Interface
summary: wunder both exposes its capabilities to other agent systems over the A2A protocol and mounts external A2A services as tools the model can call.
updated_at: 2026-10-13
---

If the other side is "another agent system" that you want to call back and forth over one shared protocol, use A2A. If the calls only happen inside your own business system, [wunder API](/docs/en/integration/wunder-api/) is simpler.

A2A works in two directions in wunder:

- Expose wunder's capabilities so external agent platforms can call them.
- Bring external A2A services in as tools the model can use.

## A2A Fit Criteria

Good fit: an external agent platform treats wunder as a remote collaborator, or cross-system collaboration needs a clear protocol boundary.

Poor fit: ordinary file operations that only run inside this system, single-machine built-in capabilities, and synchronous calls that have no reason to cross systems. Keep those local.

## External Systems Calling wunder

wunder provides these endpoints:

- `POST /a2a`: the main call entry
- `GET /.well-known/agent-card.json`
- `GET /a2a/agentCard`
- `GET /a2a/extendedAgentCard`

External systems send requests as JSON-RPC 2.0. The protocol version is 1.0, and streaming responses are supported. They first read the AgentCard to learn who wunder is, where the entry is, what skills it offers, what tool categories it supports, and whether it supports streaming; when an API key is configured, the AgentCard also declares the matching security scheme.

## wunder Calling External Services

Declare an external A2A service in the configuration, give it a name and a service address, then switch it on. Once enabled, the service appears to the model as a callable tool whose name comes from the service name, in the form `a2a@<service name>`.

The system also ships two helper tools, one for observing task status and one for waiting for a result to settle, which are handier when running long tasks.

## Key Settings and Common Pitfalls

- The service name becomes part of the tool name, so choose it carefully.
- External services of the internal type usually need a fixed user identity.
- Whether wunder is allowed to call itself is a separate switch.
- Calls have a timeout limit, so assess it in advance when the remote side is slow.

## Further Reading

- [wunder API](/docs/en/integration/wunder-api/)
- [MCP Endpoint](/docs/en/integration/mcp-endpoint/)
