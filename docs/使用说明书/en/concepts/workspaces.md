---
title: "Workspaces and Containers"
summary: "Wunder's file isolation is not 'a current directory', but a persistent workspace organized by `user_id + container_id` in layers."
read_when:
  - "You want to understand why files are isolated by user and container"
  - "You need to determine the relationship between `agent_id`, `container_id`, and workspace"
source_docs:
  - "docs/设计文档/01-系统总体设计.md"
  - "docs/API文档.md"
---

# Workspaces and Containers

Wunder's workspace is not a simple "current directory" concept, but a stable isolation strategy.

You need to distinguish at least two layers:

1. Isolate different callers by `user_id`
2. Further separate each caller's private space and agent execution space by `container_id`

## Current Conventions

- `container_id=0`: User private container
- `container_id=1~10`: Agent execution containers

## `container_id=0` Is Not Just a Private Folder

The user private container is also the root directory for "agent-visible system".

This is not only for user's own files, but also carries agent-visible configurations:

- `global/tooling.json`
- `skills/`
- `agents/<agent_id>.worker-card.json`

At runtime, a lightweight sync and validation is performed before request scheduling, and valid snapshots and diagnostics are written to `.wunder/`.

## What Are `container_id=1~10` Used For

These containers are mainly for agent runtime use, suitable for carrying:

- Task intermediate artifacts
- Single agent execution directories
- Automation process files that need to be separated from user private directories

## What Is the Relationship Between `agent_id` and Workspace

`agent_id` is not equal to workspace.

- `agent_id` determines conversation, configuration, and main thread binding
- `container_id` determines file space routing

In the current system, `agent_id` can participate in container derivation, but it does not mean "one agent corresponds to one complete private directory world".

In particular, note that:

- raw `/wunder` accepts `agent_id`
- But this field is currently mainly used for main session binding and workspace/container routing
- It does not automatically fill in the complete agent personality snapshot

## What Do External Paths Look Like

From the model and API perspective, the public view typically appears as:

- `/workspaces/{user_id}/...`

The implementation layer splits different containers into different directories, for example:

- Container 0: `/workspaces/{user_id}/`
- Container 1~10: `/workspaces/{user_id}__c__{container_id}/`

You don't need to manually remember the underlying directory names, but you need to understand: the same `user_id` can simultaneously have multiple persistent workspaces.

## Common Misconceptions

### Misconception 1: `agent_id` Equals Workspace

No.

`agent_id` determines conversation and configuration binding, `container_id` determines file space routing.

### Misconception 2: All Files Should Be in the Private Container

No.

The private container is more suitable for user-level materials and agent-visible configurations; task artifacts and automation process files are more suitable for runtime containers.

### Misconception 3: There's No Container Concept in Local Mode

Also no.

desktop/cli just maps containers to real local directories, it doesn't cancel container semantics.

## Further Reading

- [Workspace API](/docs/en/integration/workspace-api/)
- [wunder API](/docs/en/integration/wunder-api/)
- [Desktop Local Mode](/docs/en/ops/desktop-local-mode/)