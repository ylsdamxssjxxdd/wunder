---
title: Admin Interface
summary: The admin frontend lives in `web/`, handling models, tools, users, channels, and runtime governance.
read_when:
  - You want to understand the interface seen by administrators and system maintainers
  - You need to know the responsibility boundary between the admin frontend and the user frontend
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
  - docs/API文档.md
---

# Admin Interface

Wunder's admin interface is not a regular user entry point.

It is more of a governance and debugging console, serving:

- Administrators
- Platform maintainers
- Integration developers

## Code Location

- `web/`

## Default Responsibilities

The admin interface is primarily responsible for:

- Model configuration
- Tool management
- MCP and A2A service management
- User and organization governance
- Channel integration and runtime monitoring
- Preset agent governance
- Benchmark and performance observation

## Why It Must Be Separate from the User Frontend

The user frontend pursues a "workbench experience."

The admin frontend pursues:

- Viewing global state
- Changing configuration
- Performing audits and troubleshooting
- Platform governance

These two goals are fundamentally different, so Wunder explicitly splits them into two separate interfaces.

## What You Currently See in the Admin Interface

- System and model configuration pages
- Tool and MCP/A2A debugging panels
- Channel accounts and runtime monitoring
- Preset agents and sync panels
- Documentation section with usage manual and API entry points
- Operations capabilities such as benchmark, throughput, and performance sampling

## Token Accounting in User Management

Under "User Management -> Settings," regular users now support two types of Token operations:

- Set balance directly
- Issue Tokens / Deduct Tokens

These entry points have different semantics:

- **Set balance directly**: Overwrites the current balance; suitable for correcting ledger values
- **Issue Tokens**: Increases the user's held balance and records it as cumulative earnings
- **Deduct Tokens**: Decreases the user's held balance and records it as cumulative spending

Admin accounts are not subject to Token balance limits, so the relevant controls are disabled in the interface.

## Relationship Between the Usage Manual and the Admin Interface

The current usage manual artifacts are also published under `web/docs/` and served statically via `/docs/`.

This means the admin interface and the usage manual share the same static resource publishing method, but their responsibilities are different:

- The admin interface is for governance and debugging
- The usage manual is for documentation and navigation

The admin documentation section now directly embeds:

- [Usage Manual](/docs/)
- Built-in admin API pages

## When to Check the Admin Documentation First

This is useful when:

- You are integrating or troubleshooting MCP/A2A
- You need to adjust models, authentication, organizations, and Token accounts
- You want to view channel account status, errors, and bulk governance capabilities

## Further Reading

- [Deployment and Operations](/docs/en/ops/deployment/)
- [Authentication and Security](/docs/en/ops/auth-and-security/)
- [MCP Endpoint](/docs/en/integration/mcp-endpoint/)
- [Admin Panel Index](/docs/en/reference/admin-panels/)