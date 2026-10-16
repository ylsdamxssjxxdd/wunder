---
title: Workspace Routing and File Management
summary: Uniqueness and isolation of the cloud directory, how `/wunder/workspace*` is resolved, path conventions, usage statistics, and file management.
updated_at: 2026-10-16
---

Every user has a single cloud directory.

## Cloud Directory

Isolation has one layer only: by user. Each user's files live in their own single cloud directory, partitioned by the server under its data root.

| Convention | Meaning |
|------|------|
| **Unique** | There is only one workspace directory at any time; opening more threads or subagents never adds another |
| **Fixed** | You cannot add, delete, or replace the directory, and you cannot bind a folder on your machine |
| **Isolated** | Path checks are bounded by the user root; out-of-bounds access is rejected |
| **Persistent** | The workspace is persistent storage and is not cleaned up automatically |

## Routing Rules

Routing for `/wunder/workspace*`:

1. The request carries the signed-in user's identity
2. The server resolves that user's workspace root
3. The entry is located under that root by the relative path in the request

- **The container parameter has been converged**: the historical `container_id` no longer selects a directory, and the server always resolves against the user root. The parameter is still accepted during the transition, but 0 and 1 produce the same result, and the front end no longer offers a container choice.
- **No more per-agent roots**: `agent_id` only affects conversation and configuration binding, not directory derivation.
- **Subagents do not get their own directory**: a subagent is a temporary work unit created by the main agent and shares the same user directory.

## Path Conventions

- The API takes and returns **workspace-relative paths**, for example `docs/notes.md`
- The root directory is written as an empty string
- Do not pass a machine absolute path, and never show a server absolute path in the front end
- Invalid or out-of-bounds names are rejected outright rather than silently redirected

## Workspace versus Temporary Directory

| | Workspace | Temporary directory |
|--|--------|----------|
| **Purpose** | Long-lived files and task artifacts | Transit for uploads and downloads, and document-conversion intermediates |
| **Lifecycle** | Persistent, never cleaned automatically | May be cleaned periodically |
| **Paths** | Workspace-relative | A separate transit namespace |

Keep files you need to keep in the workspace and transit files in the temporary directory.

## Usage Statistics

`GET /wunder/workspace/stats` returns a bare object whose fields include the file count, directory count, bytes used, whether the walk was truncated, and recently modified files:

- The walk is bounded; it ends early for very large directories, and a true `truncated` means the counts are a lower bound
- A `null` `quota_bytes` means no quota is configured; the interface then shows only the used amount, with no quota denominator
- The working-directory overview card on the welcome page and the usage bar at the bottom of the left sidebar share this data

## File Management

- **Browse**: directories load lazily, fetching children only when expanded; large directories are paginated and rows are virtualized
- **Upload**: use the toolbar, or drag files onto the file-tree area; uploads are concurrency-limited and can be cancelled or retried
- **Preview and edit**: text, images, Office documents, and diagrams each have a preview or online-edit overlay
- **Reference to chat**: choose "Reference to chat" in a file row menu to bring the file into the composer; entries in the timeline link back to it
- **Batch and archive**: multi-select to delete in bulk or download an archive
- **High-risk operations**: deleting or clearing a directory needs a second confirmation; front-end confirmation never replaces server-side authorization

## Uploads and File Types

| Type | Handling |
|------|------|
| Documents (PDF, Word, Markdown, and so on) | Converted to text automatically for the agent to analyze |
| Images (PNG, JPG, and so on) | The agent can view and analyze them directly |
| Audio (MP3, WAV, and so on) | Transcribed to text automatically |
| Video (MP4, and so on) | Frames are extracted at a set rate for analysis |
| Code files | Files in various programming languages, which the agent can read directly |
| Other | Stored as-is, and the agent can try to read them |

Some files must be processed after upload (document-to-text, audio transcription, and so on), and the send button stays disabled until processing finishes.
