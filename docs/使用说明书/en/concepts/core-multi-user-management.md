---
title: Multi-user Management
summary: The Multi-user Management core explains why Wunder must build organization, user, permission, and token account governance into the system skeleton from day one, rather than bolting it on later.
read_when:
  - You are deploying server or designing the admin side
  - You want to understand why multi-user governance cannot be retrofitted
source_docs:
  - docs/总体设计.md
---

# Multi-user Management

Wunder targets both organizations and individuals, but once it enters server mode, multi-user governance is no longer an auxiliary feature — it is the system skeleton itself. It determines not what the backend looks like, but how the entire operational boundary is established.

![Multi-user management diagram: identity layer, governance layer, isolation layer, and result layer converging in sequence](/docs/assets/manual/core-multi-user-management.svg)

## Bottom Line

- Multi-user management is not just a "login system"; it is the sum of operational boundaries, resource boundaries, and governance boundaries.
- If data isolation, permission boundaries, and token account control are not designed upfront, you will almost certainly need to rework them later.
- The admin panel is not a showcase page — it is the formal entry point for system governance capabilities.

## Why It Must Be a Core

Without multi-user management as a core, the system will expose structural problems as it grows:

- Not knowing whether a request represents a "logged-in user" or an "externally mapped user_id."
- Threads, workspaces, tool visibility, and channels all lacking consistent boundaries.
- Token account governance and permissions can only be patched at the API layer, never truly resolved at runtime.

## What This Core Actually Protects

- Identity boundaries: distinguishing logged-in users, organization users, virtual `user_id`s, and externally mapped identities.
- Operational boundaries: threads, workspaces, tools, and resources must all know "who owns this, who can touch it."
- Governance boundaries: administrators having management privileges does not mean the system has no boundaries everywhere.

## Key Constraints

| Constraint | Purpose |
|------|------|
| Default to multi-user concurrent design | Avoid discovering later that the system only supports single-user scenarios |
| Data isolation must be designed upfront | Workspaces, sessions, configurations, and logs must have clear boundaries from the start |
| Permissions and token account governance cannot be retrofitted | Otherwise "can view but cannot manage, can invoke but cannot control" gray zones easily appear |
| Admin panel and runtime share the same governance model | What administrators see must be consistent with actual runtime behavior |

## Design Highlights

### Highlight 1: Identity and isolation are two separate layers

Logged-in users, API Key callers, externally embedded mapped identities, and virtual `user_id`s are not all the same thing. The system must distinguish "who is being authenticated" from "whose boundary is the isolation built around."

### Highlight 2: Multi-user governance will inevitably sink into the runtime

It will not stay in backend forms; it will ultimately materialize in:

- Thread ownership
- Workspace isolation
- Tool visibility
- Token accounts and resource governance

### Highlight 3: Administrator capabilities must be strong, but boundaries must remain clear

Administrators can govern the system, but this does not mean the system can lose its sense of boundaries. The healthy approach is "authorized to govern, but still traceable and limitable."

## Common Misconceptions

- The access semantics of `user_id` and backend registered users are not the same concept.
- Data isolation cannot be adequately handled by database table field patches alone.
- The admin panel is not "build features first, then assemble a UI"; it is the formal entry point for governance capabilities.

## Boundaries with Other Cores

- Difference from [Channels](/docs/en/concepts/core-channels/): channels address entry-point convergence, multi-user management addresses the governance boundaries behind those entry points.
- Difference from [Scheduled Tasks](/docs/en/concepts/core-scheduled-tasks/): scheduled tasks must also obey multi-user ownership and permissions.
- Difference from [Observability](/docs/en/concepts/core-observability/): observability must be sliceable by user and organization; the slicing rules come from multi-user management.

## Further Reading

- [Core Overview](/docs/en/concepts/)
- [Server Deployment](/docs/en/start/server/)
- [Authentication & Security](/docs/en/ops/auth-and-security/)
- [Admin Panel Index](/docs/en/reference/admin-panels/)
