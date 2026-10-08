---
title: Data and Storage
summary: wunder's persistence requires distinguishing between databases, workspaces, vector storage, and temporary directories.
read_when:
  - You need to deploy or migrate data
  - You want to understand what PostgreSQL, SQLite, workspaces, and temp_dir each store
source_docs:
  - config/wunder-example.yaml
  - docs/API文档.md
  - docs/设计文档/01-系统总体设计.md
---

# Data and Storage

When deploying wunder, the most confusing aspect is often "what should be persisted versus what's just a temporary directory."

## First, Distinguish Four Categories of Data

### Relational Primary Data

This is the core business data.

For example:

- Conversations
- Users
- Channels
- Memories
- User world messages
- Admin configurations

### Workspace Files

This is the persistent file space generated during user and agent operations.

It is not the same category as the database.

### Vector Knowledge Base

This is retrieval-related data and should not be conflated with regular conversation tables.

### Temporary Directory

This is a transit zone, not a long-term storage area.

## What `storage.backend` Determines

The configuration currently supports:

- `auto`
- `sqlite`
- `postgres`

It determines the primary business storage backend.

## Typical Choices for Hull (server) and Honeycomb (desktop)

Current best practice:

- Hull (server): Prefer PostgreSQL
- Honeycomb (desktop): Prefer SQLite

This is not a matter of style, but determined by the runtime form:

- Hull is oriented toward multi-user, multi-tenant, and sustained concurrency
- Honeycomb is more single-machine, local, and lightweight persistence

## What Goes in Vector Storage

Vector knowledge base related capabilities use the configured primary storage backend:

- PostgreSQL in Hull (server) deployments
- SQLite in Honeycomb (desktop) and local deployments

They primarily handle vector retrieval-side data such as document chunks and embedding vectors.

So don't think of it as "a replacement for the primary business database."

## What Goes in Workspaces

The workspace is **one fixed cloud directory per user**: one directory per user, partitioned by the server under its data root. Users cannot add, delete, or replace it, and cannot bind a folder from their own machine.

The workspace root is typically controlled by:

- `workspace.root`

It contains:

- User-uploaded files
- Files produced by the agent
- Skill and knowledge files
- Runtime snapshots and diagnostics

Paths handed to models and API consumers are workspace-relative; server absolute paths are never exposed. Back up and migrate the workspace as a whole per user directory, keeping the directory structure unchanged.

## What is `temp_dir`

`/wunder/temp_dir/*` corresponds to the temporary directory.

It is suitable for:

- Upload transit
- Download forwarding
- External clients fetching temporary files

It is NOT suitable for:

- Storing long-term business materials
- Using as a formal workspace

## Why Workspaces and temp_dir Must Be Separate

Because their lifecycles are different:

- Workspaces emphasize sustained reference
- temp_dir emphasizes temporary distribution and transit

If mixed together, troubleshooting and cleanup become painful.

## Typical Persistence Checklist

After deployment, at minimum confirm:

1. The primary database is your expected backend
2. The workspace directory has persistent volumes
3. The primary database is persisted because vector data is stored there
4. `temp_dir` is not being used as long-term storage

## Common Misconceptions

- Thinking SQLite and PostgreSQL are just "performance differences"
- Writing workspace outputs only to temp_dir
- Forgetting to persist `/workspaces`
- Thinking vector storage will automatically save all business data

## Further Reading

- [Deployment and Running](/docs/en/ops/deployment/)
- [Workspaces and Working Directory](/docs/en/concepts/workspaces/)
- [Configuration Reference](/docs/en/reference/config/)
