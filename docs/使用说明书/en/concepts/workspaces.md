---
title: Workspaces and the Working Directory
summary: "Each user has exactly one workspace, the root for all file tools. In the Honeycomb it is a persistent local directory; in Hull deployments it is a single per-user directory on the server."
---

# Workspaces and the Working Directory

## How it works

A wunder workspace is a file space with a clear isolation policy. Isolation has exactly one layer: **by user**.

### One layer of isolation

```
User A's workspace
├── Files the user puts in
├── Files the agent generates
├── Agent-visible configuration (skills, knowledge, global settings)
└── Runtime snapshots and diagnostics

User B's workspace (fully isolated)
└── Invisible to User A and vice versa
```

- Files of different users are fully isolated; neither side can see the other
- One user has exactly one workspace — no extra "personal files / agent files" layer
- A workspace maps 1:1 to the agent instance: one user, one agent, one workspace

### Where the workspace lives

The location depends on the form you use:

| Form | Workspace location | Access |
|------|-----------|---------|
| **Honeycomb (desktop, default)** | A persistent directory on your machine | Browse with your file manager; the agent reads and writes directly |
| **Hull (server)** | A single per-user directory partitioned under the server data root | Upload, download, and edit via the "Working directory" area in the web sidebar |

### Key rules

| Rule | Meaning |
|------|------|
| **One per user** | Each user has exactly one workspace at any time; no second directory is created for another agent |
| **Fixed** | Once assigned, the workspace is fixed — users cannot add, delete, or replace it |
| **Relative paths** | Paths shown in the UI and exchanged through APIs are always workspace-relative; absolute paths are never exposed |

## What it does

### Data isolation

- Files of different users are fully isolated; a user can only access their own workspace
- Path checks use the user root as the boundary; out-of-bounds reads and writes are rejected
- The agent's file tools are rooted at this directory; no second root is derived per agent or container

### File persistence

- The workspace is persistent storage; nothing is auto-cleaned
- Files you put in and files the agent generates are kept persistently
- Keep long-lived files in the workspace; use the temp directory for transient transfers

### Root of agent-visible configuration

The workspace is also the root of "what the agent can see":

- Skills directory
- Knowledge files
- Global settings

The agent reads these at startup. Note that **the agent's model, prompts, tools, and approval mode are managed through admin-side presets and the settings page, not by hand-writing files in the workspace**.

## In practice

### Accessing the workspace

- **Honeycomb (default)**: a persistent local directory, browsable with your file manager; the in-app "Working directory" area in the left sidebar provides the file tree and toolbar
- **Web (Hull deployments)**: the "Working directory" area below the workspace tree in the left sidebar, with a file tree and toolbar (upload / create / refresh / multi-select)
- **In conversation**: hand files to the agent, or ask it to read and write files in the workspace
- **Reference into chat**: use the file row menu's "Reference into chat" to bring a file into the input box
- **Downloading files (web)**: select files in the tree to download; multi-select bundles them into an archive

### Organizing files

**Create subdirectories by project or topic**:

- One directory per project; inside it, separate source material from output
- Put files the agent should analyze into the workspace first, then reference the path or filename in your message
- Reports, scripts, and intermediate artifacts the agent generates land in the same tree; tidy them up as needed

**Clean up regularly and name things well**:

- Delete files you no longer need to keep the directory tidy
- Use meaningful filenames so you and the agent can find them

### Usage and space

- The bottom of the file area shows used capacity, file count, and directory count
- Statistics use bounded traversal; very large directories are marked as lower bounds
- In Hull deployments with a quota configured, uploads are rejected with a prompt to clean up once the limit is hit; without a quota only used capacity is shown

### The Honeycomb's local boundary

The Honeycomb (desktop) runs locally, and its working directory is a persistent directory on your machine, not a temporary sandbox:

- Directory contents persist; no periodic cleanup
- The agent can access local paths, depending on local security settings
- More flexible, but you should understand the directory boundaries

## Common pitfalls

- **Can I switch the workspace directory?** No. The workspace is assigned per user; it cannot be replaced and you cannot add a second one.
- **More agents, more directories?** No. Each user has exactly one agent instance and one workspace.
- **Should files be split into "personal" and "agent" containers?** No. That layering has been removed; the user and the agent share the same workspace.
- **Are absolute paths faster?** Don't. APIs exchange paths relative to the workspace root; absolute paths are never exposed.

## Further reading

- [Workspace Routing Reference](/docs/en/reference/workspace-routing/) — file management tips
- [Workspace Files Tool](/docs/en/tools/workspace-files/) — how the agent reads and writes files
- [Data and Storage](/docs/en/ops/data-and-storage/) — storage policy
