---
title: A2A Interface
summary: Wunder exposes A2A JSON-RPC standard access via /a2a and provides a capability discovery entry through AgentCard.
read_when:
  - You want Wunder to be called by other agent systems
  - You want to plug external A2A services into Wunder's tool system
source_docs:
  - docs/API文档.md
  - docs/设计文档/01-系统总体设计.md
  - src/api/a2a.rs
  - config/wunder-example.yaml
---

# A2A Interface

A2A solves the problem of "how agent systems interoperate in a standardized way".

In Wunder, this pathway has two layers of meaning:

1. Wunder itself exposes an A2A service.
2. Wunder can also mount external A2A services as `a2a@service` tools for the model to call.

## Exposed Endpoints

- `POST /a2a`
- `GET /.well-known/agent-card.json`
- `GET /a2a/agentCard`
- `GET /a2a/extendedAgentCard`

## Current Protocol Characteristics

- Request protocol: JSON-RPC 2.0
- A2A protocol version: `1.0`
- Supports streaming responses
- Supports AgentCard capability discovery
- When `api_key` is configured, AgentCard also declares the API Key security scheme

## What Is AgentCard For

AgentCard is primarily used to tell external systems:

- What your service is called
- What its entry URL is
- What skills it supports
- What tool categories it supports
- Whether it supports streaming

In other words, AgentCard is the "discovery and self-introduction" layer.

## When to Use `POST /a2a`

Suitable for these scenarios:

- Letting external agent platforms treat Wunder as a remote collaborator
- Letting external systems tap into Wunder's model and tool capabilities through a unified standard
- Cross-system agent orchestration, rather than single-system tool calls only

## Difference from `/wunder`

- `/wunder` is more like Wunder's own unified execution entry
- `/a2a` is more like an external interoperability protocol entry

If you are integrating internal business system calls, start with `/wunder`.

If you are connecting to "another agent system", prefer `/a2a`.

## How to Use External A2A Inside Wunder

You can declare external A2A services in the configuration file:

```yaml
a2a:
  services:
    - name: wunder
      endpoint: http://127.0.0.1:8000/a2a
      enabled: false
```

Once enabled, this service appears on the tool side as `a2a@wunder`.

The system also includes two built-in helper tools:

- `a2a观察`
- `a2a等待`

They are used to observe task status and wait for result convergence, respectively.

## Things to Note When Integrating

- A2A service names become part of the tool name, e.g. `a2a@service_name`
- Services with `service_type=internal` typically require a fixed `user_id`
- Whether self-invocation is allowed is controlled by `allow_self`
- Timeout is controlled by `a2a.timeout_s`

## Tasks Suited for A2A

Things well-suited for A2A:

- Remote specialized capabilities
- Cross-system agent collaboration
- External agent calls requiring clear protocol boundaries

Things not suited for A2A:

- Ordinary file tools that only run within this system
- Single-machine built-in capabilities
- Synchronous calls that have no need to cross systems

## Further Reading

- [wunder API](/docs/en/integration/wunder-api/)
- [MCP Endpoint](/docs/en/integration/mcp-endpoint/)
- [Swarm Collaboration](/docs/en/concepts/swarm/)
