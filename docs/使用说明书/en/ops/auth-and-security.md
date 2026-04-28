---
title: Authentication and Security
summary: Wunder needs to distinguish between API Keys, user tokens, external auth, and tool execution boundaries.
read_when:
  - You are deploying Wunder to a long-running environment
  - You need to determine which credentials to use for which entry points
source_docs:
  - docs/API文档.md
  - docs/设计文档/01-系统总体设计.md
  - src/core/auth.rs
  - config/wunder-example.yaml
---

# Authentication and Security

Wunder has more than one identity type and entry point, so authentication is not just about "is there a token or not."

## Four Types of Credentials

### API Key

Used primarily for these entry points:

- `/wunder`
- `/a2a`
- `/wunder/mcp`
- Admin endpoints

Transmission methods:

- `X-API-Key`
- `Authorization: Bearer <api_key>`

### User Bearer Token

Used primarily for user-facing endpoints:

- `/wunder/chat/*`
- `/wunder/user_world/*`
- `/wunder/workspace/*`
- `/wunder/user_tools/*`

### External Embed Auth

Used when embedding Wunder into external systems:

- `/wunder/auth/external/*`

Corresponding configuration:

- `security.external_auth_key`

### Logged-in User Identity

This layer represents the real business identity for registered users, organizations, and organizational governance.

It is not the same concept as the arbitrary `user_id` passed into `/wunder` calls.

## An Important Distinction

The `user_id` passed to `/wunder` does not have to be a registered user.

It can simply be:

- A thread isolation identifier
- A workspace isolation identifier
- An external system mapping identifier

Real registered user management goes through the user system endpoints.

## Path Protection Boundaries

In the current implementation, you can roughly understand:

- `/a2a` is treated as a protected entry point
- `/wunder/mcp` is treated as a protected entry point
- User chat and workspace paths are user-mode endpoints
- `/docs/` serves as a static documentation site, separate from these business auth paths

The documentation is placed at `/docs/` specifically to avoid getting entangled in the `/wunder/*` auth logic.

## Tool Execution Security

Wunder's security boundaries are not only at the HTTP layer, but also at the tool layer.

Key constraints include:

- `allow_commands`
- `allow_paths`
- `deny_globs`
- Sandboxed downstream execution

In other words, even if the model can call tools, what it can actually do is still constrained by configuration.

## Thread Safety Conventions

These two conventions are very important:

- A thread's system prompt must be frozen once initially established
- Long-term memory can only be injected once during thread initialization

This is not a formality requirement, but to ensure:

- Prompt caching stability
- Predictable thread behavior
- Long-running tasks don't drift due to repeated system prompt rewrites

## WebSocket and Approval Isolation

The current system has unified pending approval requests into a shared registry, but different entry points still consume them isolated by `source`.

This means:

- Chat/WS approvals won't mistakenly clear channel approvals
- Channel-side approvals won't mistakenly operate WebSocket sessions

## Security Configuration to Check First

- `security.api_key`
- `security.external_auth_key`
- `allow_commands`
- `allow_paths`
- `deny_globs`
- Whether sandbox is enabled

## Further Reading

- [Deployment and Runtime](/docs/en/ops/deployment/)
- [Wunder API](/docs/en/integration/wunder-api/)
- [Configuration Reference](/docs/en/reference/config/)