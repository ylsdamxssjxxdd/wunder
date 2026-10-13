---
title: Data and Storage
summary: The database, workspace, vector store, and temporary directory — what each holds, what persists, and what gets cleaned up.
updated_at: 2026-10-13
---

The four kinds of data live in different places: business data in the database, user files in the workspace, the retrieval index in the vector store, and upload and download transit in the temporary directory. Backups center on the database and the workspace; the temporary directory does not need to be kept.

## The Four Kinds of Data

| Kind | Contents | Storage | Lifecycle |
| --- | --- | --- | --- |
| **Business data** | Users, sessions, configuration | Database | Persistent |
| **File data** | User files, agent output | Workspace | Persistent |
| **Vector data** | Knowledge-base retrieval | Vector store | Persistent |
| **Temporary data** | Upload and download transit | Temporary directory | Short-lived |

The distinction is between what should persist and what is only temporary transit.

## Choosing a Database

### SQLite and PostgreSQL

| | SQLite | PostgreSQL |
| --- | --- | --- |
| **Best for** | Honeycomb local mode | Hull multi-user mode |
| **Character** | Lightweight, no extra installation | Production-grade, handles high concurrency |
| **Deployment** | Created automatically | Needs a separate deployment |

- Personal use (Honeycomb desktop) → SQLite
- Team use (Hull server) → PostgreSQL

### What the Database Stores

The database holds all the core business data:

- User accounts and organization information
- Session records and message history
- Agent configuration
- Channel bindings and runtime logs
- System configuration

### Backup

- **PostgreSQL**: regular full backups plus log-based increments
- **SQLite**: copy the database file regularly
- Verify that backups can actually be restored

## Workspace

The workspace is the single fixed cloud directory per user: one directory per user, partitioned by the server under its data root. Users cannot add, delete, replace, or bind a folder from their own machine.

### What It Stores

- Files the user uploads
- Files the agent generates
- Skill and knowledge files
- Runtime snapshots and diagnostics

### Storage Notes

- **Hull mode**: the workspace must be persisted, on NAS or network storage
- **Honeycomb mode**: the workspace is a persistent directory on the user's own machine
- Don't put the workspace in the temporary directory
- Back up and migrate it as a whole per user directory, keeping the directory structure unchanged

### Space Management

- Check workspace usage regularly; the interface shows a usage bar
- Clean up unneeded files promptly
- Consider an archiving strategy for large files

## Vector Knowledge Base

The vector store handles semantic retrieval for the knowledge base, so the agent can quickly find the relevant content.

- The vector store is not a replacement for the primary database
- It only handles retrieval
- Back it up together with the primary database

## Temporary Directory

The temporary directory is the transit area for file transfer:

- Temporary holding during uploads
- Forwarding during downloads
- Intermediate files during document conversion

When managing it:

- Don't keep long-term files there
- Temporary files are cleaned up periodically
- Configure an automatic cleanup policy in production

## Data Migration

### From SQLite to PostgreSQL

When a growing team needs to move from local mode to server mode:

1. Export the SQLite data
2. Configure the PostgreSQL connection
3. Import the data into PostgreSQL
4. Migrate the workspace files
5. Verify data integrity

### Workspace Migration

1. Copy the whole workspace directory
2. Keep the directory structure unchanged
3. Update the path the configuration points to
4. Verify the files are accessible

## Common Misconceptions

| Claim that gets things wrong | What is actually true |
| --- | --- |
| SQLite and PostgreSQL differ only in performance | They suit different runtime forms |
| The temporary directory can hold long-term files | The temporary directory gets cleaned up |
| The vector store holds all data | The vector store holds only retrieval data |
| The workspace goes in the config directory | config is a runtime directory, not a data directory |

## Further Reading

- [Deployment and Running](/docs/en/ops/deployment/)
- [Workspaces and Working Directory](/docs/en/concepts/workspaces/)
- [Configuration Reference](/docs/en/reference/config/)
