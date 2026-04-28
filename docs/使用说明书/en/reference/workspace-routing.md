---
title: Workspace Routing
summary: Wunder's workspace routing is not simply directory lookup by user_id; it is determined jointly by `container_id`, agent container configuration, and scoped user_id.
read_when:
  - You are troubleshooting why files ended up in "a different workspace"
  - You want to precisely understand the routing priority of workspace endpoints
source_docs:
  - docs/API文档.md
  - src/api/workspace.rs
  - docs/设计文档/01-系统总体设计.md
---

# Workspace Routing

This page is not about "what workspaces are", but about "where a request ultimately gets routed."

## Key Points on This Page

This page only answers these questions:

- Whether `container_id` or `agent_id` takes priority
- Why the same `user_id` may correspond to multiple persistent workspaces
- Why a scoped user_id can directly access a container workspace

## Routing Priority

The current routing priority for `/wunder/workspace*` is straightforward:

1. If `container_id` is explicitly provided, route by it first
2. Otherwise, if the agent has `sandbox_container_id` configured, route to that container
3. Otherwise, fall back to the default scoped user workspace compatibility strategy

This means:

- An explicit `container_id` takes priority over `agent_id`

## Container Conventions

The current conventions are:

- `container_id=0`: user's private container
- `container_id=1~10`: agent runtime containers

So when you see "why does the same user have multiple workspaces", it is essentially the container semantics at work.

## Do Not Mix Public Paths and Underlying Paths

From the public perspective, you will typically see:

- `/workspaces/{user_id}/...`

But the implementation layer maps different containers to different actual directories.

So:

- Public paths are better suited for models and API consumers to understand
- Actual directories are better suited for server-side internal isolation

## What Is a Scoped user_id

A logged-in user can also explicitly pass a scoped `user_id`, such as a container or agent-scoped ID.

This enables advanced scenarios where the frontend or debugging tools can directly access the corresponding isolated space without having to re-derive it each time.

However, if you are a new integrator, the recommended default approach is still:

- `user_id + container_id`

Rather than constructing scoped IDs yourself.

## Common Pitfalls

### Assuming `agent_id` Always Determines the Workspace

Incorrect.

As long as `container_id` is explicitly provided, it takes priority.

### Assuming Only Uploads and Downloads Need to Consider Containers

Incorrect.

Browsing, content reading, searching, uploading, writing files, moving/copying, and archive downloads all follow the same routing logic.

### Assuming Local Mode Has No Containers

Also incorrect.

Local mode simply maps containers to actual directories; it does not eliminate container semantics.

## Implementation Recommendations

- Explicit `container_id` has the highest priority.
- `container_id=0` and `1~10` serve different purposes.
- Public paths and actual directory names are not the same concept.

## Further Reading

- [Workspaces & Containers](/docs/en/concepts/workspaces/)
- [Workspace API](/docs/en/integration/workspace-api/)
- [Data & Storage](/docs/en/ops/data-and-storage/)
