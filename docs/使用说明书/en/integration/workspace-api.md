---
title: Workspace API
summary: "/wunder/workspace* is not just a file interface - it pins every user to their own single cloud directory."
read_when:
  - "You are building workspace panels, file upload/download, or artifact callbacks"
  - "You need to know why container_id no longer has any effect"
source_docs:
  - "docs/API文档.md"
  - "docs/设计文档/01-系统总体设计.md"
---

# Workspace API

If you are building file trees, editors, upload/download, or artifact panels, read this page first.

`/wunder/workspace*` is not just a file read/write interface - it determines which user directory a file lands in.

## Key Points on This Page

- What workspace-related interfaces are available
- How routing converges on "one directory per user"
- When to use the workspace instead of `temp_dir`

## Routing Rules (Read First)

Workspace routing now has exactly one path:

1. The request carries the authenticated user's identity.
2. The server resolves that user's workspace root.
3. The entry is located under that root by the relative path in the request.

Additional notes:

- `container_id` is converged: the parameter is still accepted during the transition, but it no longer selects a directory — any value resolves against the user root. The frontend offers no container choice.
- `agent_id` no longer participates in directory derivation; it only affects conversation and configuration binding. Each user has exactly one agent instance.
- Subagents share the same user directory as the main agent.

## Where This Set of Interfaces Is Used

- File tree and directory browsing
- File preview and editing
- Upload, download, and archive packaging
- Persistent callback of tool artifacts
- Directory usage statistics (file count, directory count, used capacity, recently modified files)

## How to Distinguish Common Interfaces

- `GET/DELETE /wunder/workspace`
- `GET /wunder/workspace/content`
- `GET /wunder/workspace/search`
- `GET /wunder/workspace/stats`
- `POST /wunder/workspace/upload`
- `GET /wunder/workspace/download`
- `GET /wunder/workspace/archive`
- `POST /wunder/workspace/dir`
- `POST /wunder/workspace/move`
- `POST /wunder/workspace/copy`
- `POST /wunder/workspace/batch`
- `POST /wunder/workspace/file`

If you are building a file panel, you can understand it this way:

- Directory page: `GET /wunder/workspace`
- File preview: `GET /wunder/workspace/content`
- Search: `GET /wunder/workspace/search`
- Usage bar and welcome-page overview: `GET /wunder/workspace/stats`
- Write file: `POST /wunder/workspace/file`
- Upload: `POST /wunder/workspace/upload`
- Export: `GET /wunder/workspace/download` or `archive`

The `/workspace*` group returns bare objects (no `data` wrapper). In `stats`, `quota_bytes: null` means no quota is configured — the frontend then shows only the used amount and renders no quota denominator.

## Common Misconceptions

- Treating the workspace as a temporary directory. The workspace is for persistent artifacts; `temp_dir` is for transient transfers.
- Passing real disk absolute paths directly. Every interface here uses workspace-relative paths.
- Thinking `container_id` can switch to another directory. It cannot: the server always resolves against the user root.
- Thinking the workspace can be added or replaced. It cannot: each user has one fixed cloud directory.

## Further Reading

- [Workspaces and Working Directory](/docs/en/concepts/workspaces/)
- [Workspace Routing and File Management](/docs/en/reference/workspace-routing/)
- [Temp Directory and Document Conversion](/docs/en/integration/temp-dir/)
