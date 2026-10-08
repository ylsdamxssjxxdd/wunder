---
title: Workspace Routing and File Management
summary: Each user's workspace is a single cloud directory. Paths are always workspace-relative, and container-based routing has been converged away.
read_when:
  - "You are building file trees, upload/download, or artifact callbacks"
  - "You need to know where a path actually lands, or why a file cannot be found"
source_docs:
  - docs/API文档.md
  - docs/设计文档/01-系统总体设计.md
---

# Workspace Routing and File Management

This page answers two questions: **where a file lands**, and **how to manage it**.

## Key points on this page

- How many workspaces one user has
- The routing rules for `/wunder/workspace*`
- How to write paths that are not rejected
- How the workspace differs from the temporary directory

## One cloud directory per user

Isolation has exactly one layer: by user. Every user's files live in **their own single cloud directory**, partitioned by the server under its data root.

| Convention | Meaning |
|------|------|
| **Unique** | There is one workspace directory at any time; opening more threads or subagents never creates another |
| **Fixed** | You cannot add, delete, or replace the directory, and you cannot bind a folder on your machine |
| **Isolated** | Path checks are bounded by the user root; out-of-bounds access is rejected |
| **Persistent** | The workspace is persistent storage and is not cleaned up automatically |

## Routing rules

Routing for `/wunder/workspace*` is now simple:

1. The request carries the authenticated user's identity.
2. The server resolves that user's workspace root.
3. The entry is located under that root by the relative path in the request.

Additional notes:

- **The container parameter is converged**: the historical `container_id` no longer selects a directory — the server always resolves against the user root. The parameter is still accepted during the transition, but 0 and 1 produce the same result, and the frontend no longer offers a container choice.
- **No more per-agent roots**: the agent dimension no longer participates in directory derivation; `agent_id` only affects conversation and configuration binding.
- **Subagents do not get their own directory**: a subagent is a temporary work unit created by the main agent and shares the same user directory.

## Path conventions

- The API takes and returns **workspace-relative paths**, for example `docs/plan.md`.
- The root directory is expressed as an empty string.
- Never pass a machine absolute path, and never render a server absolute path in the UI.
- Invalid or out-of-bounds names are rejected outright rather than silently redirected.

## Workspace versus temporary directory

| | Workspace | Temporary directory |
|--|--------|----------|
| **Purpose** | Long-lived files and task artifacts | Upload/download transit, document conversion intermediates |
| **Lifecycle** | Persistent, never cleaned automatically | May be cleaned periodically |
| **Paths** | Workspace-relative | A separate transit namespace |

**Important**: never put files you need to keep in the temporary directory.

## Usage statistics

`GET /wunder/workspace/stats` returns a bare object with file count, directory count, used bytes, whether the walk was truncated, and recently modified files:

- The walk is bounded; for very large directories it ends early, and `truncated: true` means the counts are lower bounds.
- `quota_bytes: null` means no quota is configured — the UI then shows only the used amount, with no quota denominator.
- The welcome-page directory overview card and the usage bar at the bottom of the left sidebar share this data.

## File management in practice

- **Browse**: directories load lazily — children are fetched only when expanded; large directories are paginated and rows are virtualized.
- **Upload**: use the toolbar, or drag files onto the file tree. Uploads are concurrency-limited and can be cancelled or retried.
- **Preview and edit**: text, images, Office documents, and diagrams each have a preview or online-edit overlay.
- **Reference to chat**: use "Reference to chat" in a file row menu to bring the file into the composer; timeline entries link back to it.
- **Batch and archive**: multi-select enables batch delete or archive download.
- **High-risk operations**: delete and clear-directory require confirmation; frontend confirmation never replaces server-side authorization.

## Uploads and file types

| Type | Handling |
|------|------|
| Documents (PDF, Word, Markdown, etc.) | Converted to text automatically for the agent to analyze |
| Images (PNG, JPG, etc.) | The agent can view and analyze them directly |
| Audio (MP3, WAV, etc.) | Transcribed to text automatically |
| Video (MP4, etc.) | Frames are extracted at a configured rate for analysis |
| Code files | Any programming language file; the agent can read it directly |
| Other | Stored as-is; the agent can attempt to read it |

Some files must be processed after upload (document-to-text, audio transcription). The send button is disabled until processing finishes — that is a normal protection mechanism.

## Common pitfalls

- **Adding `container_id` to write somewhere else.** It will not: the server resolves against the user root.
- **Expecting to swap the directory or bind a local folder.** You cannot; the cloud workspace is assigned by the server.
- **Using the workspace as a temporary directory.** Keep persistent artifacts in the workspace and transit in the temp directory.
- **Passing a real disk absolute path.** This API uses workspace-relative paths.
- **Assuming files are cleaned up automatically.** The workspace is not; only the temporary directory is.

## Further reading

- [Workspaces and Working Directory](/docs/en/concepts/workspaces/)
- [Workspace API](/docs/en/integration/workspace-api/)
- [Temp Directory and Document Conversion](/docs/en/integration/temp-dir/)
- [Data & Storage](/docs/en/ops/data-and-storage/)
