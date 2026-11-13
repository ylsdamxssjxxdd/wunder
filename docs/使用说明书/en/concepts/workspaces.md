---
title: Workspaces and the Working Directory
summary: Per-user workspace isolation, the difference between the Cloud's single directory and the Desktop's multiple roots, location and rules, file operations, usage statistics, and the local boundary.
updated_at: 2026-10-16
---

In a Server deployment every user has exactly one cloud workspace, and the agent's file tools are rooted at it. The Desktop and Android can register several workspaces (local directories, the cloud workspace, and remote device workspaces reached through device interlink) and resolve each root by `workspace_id`. Tool execution is always bounded by the path fence.

## Isolation Policy

Isolation has exactly one layer: per user.

```
User A's workspace
├── The user's files
├── Files the agent generates
├── Agent-visible configuration (skills, knowledge, global settings)
└── Runtime snapshots and diagnostics

User B's workspace (fully isolated, invisible to A and vice versa)
```

- Files of different users are fully isolated; neither side can see the other.
- Server: a user has exactly one cloud workspace; there is no second "personal files / agent files" layer, and user, agent instance, and cloud workspace map one to one.
- Desktop / Android: the same user may own several workspace roots, but each root is still bounded by that user and never shared across users.

## Location

| Form | Location | How to access |
| --- | --- | --- |
| Desktop (default) | A persistent directory on your machine; several local / cloud / interlink remote workspaces can be registered at once | Browse with your system file manager; the agent reads and writes directly |
| Server | A single per-user directory under the server data root | Upload, download, and edit through the "Working directory" area in the Desktop's left sidebar |

Three firm rules:

| Rule | Meaning |
| --- | --- |
| Unique in the Cloud | In a Server deployment each user has exactly one cloud workspace at a time; no second directory is opened for another agent |
| Multiple roots on the Desktop | The Desktop / Android can add and switch several workspace roots; one tool call only acts on the current workspace |
| Fixed | In a Server deployment the cloud directory is fixed once assigned; a user cannot add, delete, or replace it |
| Relative paths | Paths shown in the interface and exchanged through APIs are always workspace-relative; absolute paths are never exposed |

## What It Carries

- **Data isolation**: a user can only access their own workspace; path checks use the user root as the boundary, and out-of-bounds reads and writes are rejected; in a Server deployment the agent's file tools are rooted at that user's cloud directory and no second root is derived.
- **File persistence**: the workspace is not cleaned automatically, so the files put in and the files the agent generates are kept long term. Long-lived files go in the workspace and transient transfers in the temporary directory.
- **Root of configuration**: the workspace is also the root of "what the agent can see" — the skills directory, knowledge files, and global settings, which the agent reads at startup to initialize. The agent's model, prompts, tools, and approval mode are bound through admin-side presets and managed on the settings page, not by hand-writing files in the workspace.

## File Operations

- **Access**: in the Desktop, use the file manager directly, or the "Working directory" area in the app's left sidebar with its file tree and toolbar. In the Desktop's left sidebar, below the workspace, there is a "Working directory" area with a file tree and an upload / create / refresh / multi-select toolbar.
- **In conversation**: hand files to the agent, or have it read and write files in the workspace directly.
- **Reference into chat**: pick "Reference into chat" from a file row's menu to bring the file into the input area.
- **Download (Desktop)**: select files in the tree to download; multi-select bundles them into an archive.

File organization: create subdirectories by project or topic, one directory per project, and split source material from output inside it; put files the agent should analyze into the workspace first, then mention the path or filename in the message; the reports and scripts the agent generates land in the same tree. Delete files no longer needed and give files meaningful names.

## Usage Statistics

- The bottom of the file area shows used capacity, file count, and directory count.
- Statistics use bounded traversal and are marked as a lower bound for very large directories.
- On a Server deployment with a quota configured, uploads are rejected with a prompt to clean up once the limit is hit; without a quota, only used capacity is shown, with no quota denominator.

## The Desktop's Local Boundary

The Desktop runs locally, and its working directory is a persistent directory on your machine rather than a temporary sandbox:

- Contents persist and are not periodically cleaned.
- Whether the agent can access local paths depends on local security settings.
- It is flexible, but the directory boundaries need checking first.

## Limits

- In a Server deployment the cloud workspace cannot be moved to another directory, nor can a second one be added; the Desktop / Android can register several workspace roots.
- The agent instance and the cloud workspace are still one-to-one: each user has one agent instance; the Server has one cloud workspace, while the Desktop can have multiple roots.
- Files no longer split into a "personal container" and an "agent container"; that layering is gone, and the user and the agent share the same workspace.
- The API exchanges paths relative to the workspace root, and absolute paths are not exposed.
