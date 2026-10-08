---
title: "Workspaces and Working Directory"
summary: "Each user has exactly one workspace: a single cloud directory under the server data root. There are no container layers and no local folder binding."
read_when:
  - "You want to understand why files are isolated by user only"
  - "You need to know the relationship between a user, an agent instance, and the working directory"
source_docs:
  - "docs/设计文档/01-系统总体设计.md"
  - "docs/API文档.md"
---

# Workspaces and Working Directory

wunder's workspace is not a simple "current directory" concept, and it is no longer layered by container. It is one directory per user.

## One layer of isolation

Isolation is by `user_id` only:

```
User A's workspace (one cloud directory)
├── Files uploaded by the user
├── Files produced by the agent
├── Agent-visible configuration (skills, knowledge, global settings)
└── Runtime snapshots and diagnostics

User B's workspace (fully isolated)
└── Invisible to user A
```

- Different users are completely isolated from each other.
- A user has exactly one workspace — there is no second layer splitting "personal files" from "agent files".
- A workspace maps 1:1 to the agent instance: one user, one agent, one cloud directory.

## Key conventions

| Convention | Meaning |
|------|------|
| **Unique** | A user has exactly one workspace directory at any time; the server does not open another one per agent |
| **Fixed** | The directory is partitioned by user under the server data root; you cannot add, delete, or replace it |
| **No local binding** | The cloud workspace always lives on the server and never maps to a folder on your machine |
| **Relative paths** | Paths shown in the UI and carried by the API are always workspace-relative; server absolute paths are never exposed |

## What it is for

### Data isolation

- Files of different users are fully isolated; every user can only reach their own workspace.
- Path checks are bounded by the user root, so out-of-bounds reads and writes are rejected.
- File tools use this user directory as their root; they no longer derive a second root per agent or container.

### File persistence

- The workspace is persistent storage and is not cleaned up automatically.
- Files you upload and files the agent produces are both kept.
- Keep long-lived material in the workspace; use the temporary directory only for transit.

### Root of agent-visible configuration

The workspace is also the root of the "agent-visible" configuration:

- the skills directory
- knowledge files
- global settings

The agent reads these when it starts. Note that the agent's model, prompt, tools, and approval mode are managed through the admin-side preset binding and the settings page — not by hand-writing files into the workspace.

## In practice

### Reaching the workspace

- **Web**: the "Working directory" area below the workspace tree in the left sidebar, with a file tree and a toolbar (upload / new / refresh / multi-select)
- **Local form**: Desktop uses a persistent directory on your own machine, viewable in your system file manager
- **In conversation**: upload files, or let the agent read and write files
- **Reference to chat**: use "Reference to chat" in a file row menu to bring the file into the composer
- **Download**: pick a file in the tree to download it, or multi-select to download an archive

### How to organize files

**Create subdirectories per project or topic**:

- one directory per project, with materials and outputs inside
- upload files the agent should analyze, then mention the path or file name in your message
- reports, scripts, and intermediate artifacts the agent produces land in the same tree, ready for you to tidy up

**Clean up and name things well**:

- delete files you no longer need, so the tree stays readable
- give files meaningful names so both you and the agent can find them

### Usage and space

- The bottom of the file area shows used capacity, file count, and directory count.
- Statistics use a bounded walk; for very large directories the count is labelled as a lower bound.
- If the server configures a quota, uploads are rejected past the limit with a prompt to clean up. Without a quota only the used amount is shown — no quota denominator.

### How the local form differs

Desktop runs locally, so its working directory is a persistent directory on your machine rather than a temporary sandbox:

- contents persist and are not subject to periodic cleanup
- the agent can reach local paths, subject to local security settings
- this is more flexible, but you need to be clear about directory boundaries

## Common misconceptions

- **Can I switch the workspace directory?** No. The cloud workspace is assigned per user by the server; it cannot be replaced and you cannot add a second one.
- **Do more agents mean more working directories?** No. Each user has exactly one agent instance and exactly one working directory.
- **Should files be split between a "personal container" and an "agent container"?** No. That layering is gone; you and the agent share the same user directory.
- **Is an absolute path faster?** Don't use one. The API takes workspace-relative paths, and absolute paths are never exposed.

## Further reading

- [Workspace API](/docs/en/integration/workspace-api/)
- [Workspace Files Tool](/docs/en/tools/workspace-files/)
- [Data & Storage](/docs/en/ops/data-and-storage/)
