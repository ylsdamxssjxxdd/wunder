---
title: System Settings Reference
summary: The six settings categories, configuration layers and effect timing, and a configuration comparison across two deployment forms.
updated_at: 2026-10-16
---

Settings fall into six categories by responsibility:

- **Basic settings**: service address, port, and run mode
- **Security settings**: access control and permission boundaries
- **Storage settings**: where and how data is stored
- **Tool settings**: which tools are available
- **Model settings**: AI model selection and parameters
- **Channel settings**: external platform access

New settings take effect for new sessions immediately; existing sessions do not change and keep the settings from when they were created. The three layers, bottom to top, are **system defaults** (the bottom layer that any unset value falls back to), **per-user settings** (override the system defaults and affect only that user), and **agent settings** (the top layer, affecting only that agent). The order of precedence is agent settings > user settings > system defaults.

## Basic settings

### Service address and port

For local use, keep the defaults; for a team deployment, set an address the team can reach.

### Run mode

Two values: **Pupa local mode** (all data stored locally) and **Hull mode** (multi-user support and centralized management). Pick Pupa local mode for individual use, and Hull mode when you need multi-user collaboration or centralized management.

### Concurrency limit

A cap on how many sessions can be active at once. The default is enough for individual use; for team use, adjust it to the number of users and the available hardware.

## Security settings

### API key

The credential guarding access to the Bridge and the Hull interfaces. The key should be long enough — over 16 characters, mixing letters, digits, and symbols; do not use a key that is easy to guess, and rotate the key regularly.

### Path access control

Limits the file paths the agent may touch and the commands it may run:

- **Allowed paths**: the directories the agent may work in
- **Denied patterns**: the file types or paths that are off limits

Do not open the whole filesystem: name the working directories the agent may use and block sensitive system directories.

### External auth key

Used to verify external requests when wunder is embedded in another system, or when another system calls into wunder's capabilities.

## Storage settings

### Database type

- **SQLite**: a lightweight local database, suited to Pupa local mode (default)
- **PostgreSQL**: a full-scale database, suited to the Hull multi-user mode

### Data directory

`storage.data_root` is the shared root for execution history, dedicated SQLite databases and attachment resources. Override it with `WUNDER_STORAGE_DATA_ROOT`. Pupa and Helm default to the `data/` directory in the local workbench home. Local state uses `state_1.sqlite`; canonical thread history uses JSONL files below `sessions/`, with a separate SQLite projection for paginated history.

Thread execution history always uses the local canonical ThreadStore. `storage.thread_store.compression_enabled` enables cold rollout compression and defaults to `false`; its environment variable is `WUNDER_THREAD_STORE_COMPRESSION_ENABLED`. The single `storage.db_path` field and `WUNDER_SQLITE_DB_PATH` variable have been removed.

### Workspace location

Where user files and agent artifacts are stored. In Hull mode, make sure the workspace directory is persistent (use NAS or network storage); do not put the workspace in a temporary directory.

### Vector storage

The storage backend used by knowledge-base retrieval, saved alongside the main storage backend: PostgreSQL in Hull mode and SQLite in Pupa local mode. Configure it when you need knowledge-base retrieval.

## Tool settings

### Built-in tool switches

Controls which built-in tools the agent may use. In sensitive environments, consider turning off command-execution tools and opening only what is actually needed.

### Browser tools

Once enabled, the agent can browse pages and take screenshots. Enable them directly for local use; a Hull deployment may need Docker container support.

### MCP extension tools

External tool services connected over the MCP protocol. Use them for specialized database operations, particular API calls, or third-party services.

## Model settings

### Available model list

The AI models the agent can choose from.

- Adding a model means supplying its API key and endpoint
- Different models have different billing and different strengths
- Offering several options lets users pick what fits

### Context limit

A cap on the conversation context length. Setting it to the model's maximum is fine; when the limit is reached, the system compresses older content automatically.

## Channel settings

### Channel account binding

Binds accounts on external platforms such as Feishu and WeCom. Create an app on the platform, obtain its credentials, and fill in the configuration for that channel type.

### Webhook configuration

The receiving address external platforms push messages to. Set up a publicly reachable address and configure the webhook URL on the external platform.

## Deployment forms

| Item | Individual local use (Pupa desktop) | Team collaboration (Hull server) |
| --- | --- | --- |
| Database | SQLite (default) | PostgreSQL |
| Workspace | A local directory | Network storage (persistent) |
| Security | Limit sensitive paths reasonably | Configure keys and path limits strictly |
| Tools | Open them as needed | Assign permissions by role |
