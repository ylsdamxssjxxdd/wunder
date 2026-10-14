---
title: Workspaces and the Working Directory
summary: Each user has exactly one workspace, and tools are rooted at it: a persistent local directory in the Pupa, or a single per-user directory on the server in a Hull deployment.
updated_at: 2026-10-13
---

A workspace is a file space with a clear isolation policy. Isolation has exactly one layer: **per user**.

## One layer of isolation

```
User A's workspace
├── The user's files
├── Files the agent generates
├── Agent-visible configuration (skills, knowledge, global settings)
└── Runtime snapshots and diagnostics

User B's workspace (fully isolated, invisible to A and vice versa)
```

- Files of different users are fully isolated; neither side can see the other.
- A user has exactly one workspace; there is no second "personal files / agent files" layer.
- User, agent instance, and workspace map one to one: one user, one agent, one workspace.

## Where the workspace lives

| Form | Location | How to access |
| --- | --- | --- |
| Pupa (default) | A persistent directory on your machine | Browse with your system file manager; the agent reads and writes directly |
| Hull | A single per-user directory under the server data root | Upload, download, and edit through the "Working directory" area in the Pupa's left sidebar |

Three firm rules:

| Rule | Meaning |
| --- | --- |
| Unique | Each user has exactly one workspace at a time; no second directory is opened for another agent |
| Fixed | Once assigned it is fixed; a user cannot add, delete, or replace it |
| Relative paths | Paths shown in the interface and exchanged through APIs are always workspace-relative; absolute paths are never exposed |

## What it carries

- **Data isolation**: a user can only access their own workspace; path checks use the user root as the boundary, and out-of-bounds reads and writes are rejected; the agent's file tools are rooted at this directory and no second root is derived.
- **File persistence**: the workspace is not cleaned automatically, so the files you put in and the files the agent generates are kept long term. Keep long-lived files in the workspace and transient transfers in the temporary directory.
- **Root of configuration**: the workspace is also the root of "what the agent can see" — the skills directory, knowledge files, and global settings, which the agent reads at startup to initialize. Note that **the agent's model, prompts, tools, and approval mode are bound through admin-side presets and managed on the settings page**, not by hand-writing files in the workspace.

## How to use it

- **Access**: in the Pupa, use the file manager directly, or the "Working directory" area in the app's left sidebar with its file tree and toolbar. In the Pupa's left sidebar, below the workspace, there is a "Working directory" area with a file tree and an upload / create / refresh / multi-select toolbar.
- **In conversation**: hand files to the agent, or have it read and write files in the workspace directly.
- **Reference into chat**: pick "Reference into chat" from a file row's menu to bring the file into the input area.
- **Download (Pupa)**: select files in the tree to download; multi-select bundles them into an archive.

Organizing tips: create subdirectories by project or topic, one directory per project and split source material from output inside it; put files the agent should analyze into the workspace first, then mention the path or filename in your message; the reports and scripts the agent generates also land in the same tree, to be tidied as needed. Delete files you no longer need and give files meaningful names.

## Usage and space

- The bottom of the file area shows used capacity, file count, and directory count.
- Statistics use bounded traversal and are marked as a lower bound for very large directories.
- In a Hull deployment with a quota configured, uploads are rejected with a prompt to clean up once the limit is hit; without a quota, only used capacity is shown, with no quota denominator.

## The Pupa's local boundary

The Pupa runs locally, and its working directory is a persistent directory on your machine rather than a temporary sandbox:

- Contents persist and are not periodically cleaned.
- Whether the agent can access local paths depends on local security settings.
- It is more flexible, but you should understand the directory boundaries.

## Common mistakes

- A workspace cannot be moved to another directory, nor can a second one be added.
- More agents do not mean more working directories: each user has one agent instance and one workspace.
- Files no longer split into a "personal container" and an "agent container"; that layering is gone, and the user and the agent share the same workspace.
- Do not write absolute paths: the API exchanges paths relative to the workspace root, and absolute paths are not exposed.

## Further reading

- [Workspace management reference](/docs/en/capabilities/workspace-routing/) — file management tips
- [Workspace files tool](/docs/en/tools/workspace-files/) — how the agent reads and writes files
