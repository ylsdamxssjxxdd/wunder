---
title: Workspace API
summary: The workspace does more than file read and write — it pins every user to their own single cloud directory.
updated_at: 2026-10-13
---

This page applies to file trees, editors, upload/download, and artifact panels.

`/wunder/workspace*` is more than file read and write: it decides whose user directory a file lands in.

## Workspace vs. Temp Directory

- An artifact you want to keep long-term → the workspace.
- Transit, or a temporary download link → the [temp directory](/docs/en/integration/temp-dir/).

## How Routing Is Decided

There is only one path: the request carries the signed-in user's identity → the server resolves that user's workspace root → it locates the entry under that root by relative path. Every endpoint uses workspace-relative paths and never exposes the server's real directories.

Boundaries:

- During the transition the container ID parameter is still accepted, but it no longer decides the directory — any value resolves against the user root, and the frontend offers no container choice.
- The agent ID does not take part in directory derivation; it only affects session and configuration binding. Each user has exactly one agent instance.
- Subagents share the same user directory as the main agent.
- Each user has one fixed cloud directory that cannot be added or replaced.

## Common Endpoints

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

When building a file panel, map them like this:

- Directory page: `GET /wunder/workspace`
- File preview: `GET /wunder/workspace/content`
- Search: `GET /wunder/workspace/search`
- Usage bar and overview: `GET /wunder/workspace/stats`
- Write file: `POST /wunder/workspace/file`
- Upload: `POST /wunder/workspace/upload`
- Export: `GET /wunder/workspace/download` or `archive`

## Key Settings and Common Pitfalls

- This group of endpoints returns bare objects directly, with no data wrapper.
- When no quota is configured in usage statistics, the quota field is empty and the frontend shows only the amount used, rendering no quota denominator.
- Do not use the workspace as a temp directory; the workspace suits persistent artifacts.
- Do not pass real disk absolute paths — everything here uses workspace-relative paths.
- Passing a container ID will not switch to another directory.

## Further Reading

- [Workspaces and Working Directory](/docs/en/concepts/workspaces/)
- [Workspace Routing and File Management](/docs/en/reference/workspace-routing/)
- [Temp Directory and Document Conversion](/docs/en/integration/temp-dir/)
