---
title: Workspace API
summary: "/wunder/workspace* is not just a file interface - it determines which isolated workspace files are stored in."
read_when:
  - "You are building workspace panels, file upload/download, or artifact callbacks"
  - "You need to understand why container_id overrides agent_id"
source_docs:
  - "docs/API文档.md"
  - "src/api/workspace.rs"
  - "docs/设计方案.md"
---

# Workspace API

If you are building file trees, editors, upload/download, or artifact panels, read this page first.

`/wunder/workspace*` is not just a file read/write interface - it determines which isolated space files are stored in.

## Key Points on This Page

- What workspace-related interfaces are available
- How `user_id`, `agent_id`, and `container_id` jointly determine routing
- When to use workspace instead of `temp_dir`

## Routing Rules (Read First)

Workspace routing priority is currently clear:

1. Explicitly passed `container_id`
2. Otherwise, check `sandbox_container_id` bound to `agent_id`
3. Then fall back to default user workspace or legacy compatibility routing

So when you explicitly pass `container_id`, its priority is higher than `agent_id`.

## Where This Set of Interfaces Is Used

- File tree and directory browsing
- File preview and editing
- Upload, download, and archive packaging
- Persistent callback of tool artifacts
- File space isolation by container for different agents

## How to Distinguish Common Interfaces

- `GET/DELETE /wunder/workspace`
- `GET /wunder/workspace/content`
- `GET /wunder/workspace/search`
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
- Write file: `POST /wunder/workspace/file`
- Upload: `POST /wunder/workspace/upload`
- Export: `GET /wunder/workspace/download` or `archive`

## Common Misconceptions

- Treating workspace as a temporary directory. Workspace is for persistent artifacts; `temp_dir` is for transient transfers.
- Passing real disk absolute paths directly. Most interfaces here use relative workspace paths.
- Thinking only download interface needs `container_id`. In fact, the entire `/wunder/workspace*` group supports explicit `container_id`.

## Further Reading

- [Workspaces and Containers](/docs/en/concepts/workspaces/)
- [Workspace Routing Reference](/docs/en/reference/workspace-routing/)
- [Temp Directory and Document Conversion](/docs/en/integration/temp-dir/)