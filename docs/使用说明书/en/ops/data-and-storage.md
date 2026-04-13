---
title: Data and Storage
summary: Wunder's persistence requires distinguishing between databases, workspaces, vector storage, and temporary directories.
read_when:
  - You need to deploy or migrate data
  - You want to understand what PostgreSQL, SQLite, Weaviate, workspaces, and temp_dir each store
source_docs:
  - config/wunder-example.yaml
  - docs/API文档.md
  - docs/设计方案.md
  - docs/系统介绍.md
---

# Data and Storage

When deploying Wunder, the most confusing aspect is often "what should be persisted versus what's just a temporary directory."

This page resolves exactly that.

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

## Typical Choices for Server and Desktop

Current best practice:

- Web / server: Prefer PostgreSQL
- Desktop local mode: Prefer SQLite

This is not a matter of style, but determined by the runtime form:

- Server is oriented toward multi-user, multi-tenant, and sustained concurrency
- Desktop is more single-machine, local, and lightweight persistence

## What Goes in Weaviate

Vector knowledge base related capabilities currently use:

- `vector_store.weaviate`

It primarily handles vector retrieval-side data.

So don't think of it as "a replacement for the primary business database."

## What Goes in Workspaces

The workspace root is typically controlled by:

- `workspace.root`

It contains:

- User private files
- Agent container files
- Task outputs
- Persisted results that can be further processed by subsequent tools

If a database task exports to `/workspaces/{user_id}/...`, it will also end up here.

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
3. The vector store has persistent volumes
4. `temp_dir` is not being used as long-term storage

## Common Misconceptions

- Thinking SQLite and PostgreSQL are just "performance differences"
- Writing workspace outputs only to temp_dir
- Forgetting to persist `/workspaces`
- Thinking Weaviate will automatically save all business data

## Further Reading

- [Deployment and Running](/docs/en/ops/deployment/)
- [Workspaces and Containers](/docs/en/concepts/workspaces/)
- [Configuration Reference](/docs/en/reference/config/)