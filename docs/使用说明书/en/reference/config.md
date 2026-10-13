---
title: System Settings Reference
summary: What the common settings mean and what to set them to.
updated_at: 2026-10-13
---

## Settings categories

wunder's settings fall into a few broad categories by responsibility:

- **Basic settings**: service address, port, and run mode
- **Security settings**: access control and permission boundaries
- **Storage settings**: where and how data is stored
- **Tool settings**: which tools are available
- **Model settings**: AI model selection and parameters
- **Channel settings**: external platform access

## Basic settings

### Service address and port

These decide the network address and port the wunder service listens on.

- For local use, the defaults are fine
- For a team deployment, set an address the team can reach

### Run mode

This decides the form wunder runs in.

- **Honeycomb local mode**: suited to individual local use, with all data stored locally
- **Hull mode**: suited to teams and organizations, with multi-user support and centralized management

Pick Honeycomb local mode for individual use; pick Hull mode when you need multi-user collaboration and centralized management.

### Concurrency limit

This caps how many sessions can be active at once, so the system does not get overloaded.

- For individual use, the default is enough
- For team use, adjust it to the number of users and the available hardware

## Security settings

### API key

This protects access to the Bridge and the Hull interfaces. A few points:

- The key should be long enough — over 16 characters, mixing letters, digits, and symbols
- Do not use a key that is easy to guess
- Rotate the key regularly

### Path access control

This limits the file paths the agent may touch and the commands it may run.

- **Allowed paths**: the directories the agent may work in
- **Denied patterns**: the file types or paths that are off limits

As a safety practice, do not open the whole filesystem: name the working directories the agent may use and block sensitive system directories.

### External auth key

When wunder is embedded in another system, this key verifies incoming external requests. It fits cases such as calling wunder from another system or an embedded integration.

## Storage settings

### Database type

This decides which database stores business data.

- **SQLite**: a lightweight local database, the default for Honeycomb local mode
- **PostgreSQL**: a full-scale database, suited to the Hull multi-user mode

### Workspace location

This decides where user files and agent artifacts are stored. A few points:

- In Hull mode, make sure the workspace directory is persistent (use NAS or network storage)
- Do not put the workspace in a temporary directory

### Vector storage

This is the storage backend used by knowledge-base retrieval, saved alongside the main storage backend: PostgreSQL in Hull mode and SQLite in Honeycomb local mode. Configure it when you need knowledge-base retrieval.

## Tool settings

### Built-in tool switches

These control which built-in tools the agent may use. In sensitive environments, consider turning off command-execution tools and opening only what is actually needed.

### Browser tools

These let the agent browse pages and take screenshots. Enable them directly for local use; a Hull deployment may need Docker container support.

### MCP extension tools

These connect external tool services over the MCP protocol. Use them when you need specialized database operations, particular API calls, or third-party services.

## Model settings

### Available model list

This decides which AI models the agent can choose from.

- Adding a model means supplying its API key and endpoint
- Different models have different billing and different strengths
- Offering several options lets users pick what fits

### Context limit

This caps the conversation context so it stays within what the model handles. Setting it to the model's maximum is fine; when the limit is reached, the system compresses older content automatically.

## Channel settings

### Channel account binding

This binds accounts on external platforms such as Feishu and WeCom. Create an app on the platform, obtain its credentials, and fill in the configuration for that channel type.

### Webhook configuration

This lets an external platform push messages to wunder. Set up a publicly reachable receiving address and configure the webhook URL on the external platform.

## When configuration takes effect

### Timing of changes

- **New settings**: take effect for new sessions immediately
- **Existing sessions**: do not change; they keep the settings from when they were created

### Configuration layers

- **System defaults**: the bottom layer that any unset value falls back to
- **Per-user settings**: override the system defaults and affect only that user
- **Agent settings**: the top layer, affecting only that agent

**Order of precedence**: agent settings > user settings > system defaults.

## Recommended configuration by scenario

### Individual local use (Honeycomb desktop)

- Database: SQLite (default)
- Workspace: a local directory
- Security: limit sensitive paths reasonably
- Tools: open them as needed

### Team collaboration (Hull server)

- Database: PostgreSQL
- Workspace: network storage (persistent)
- Security: configure keys and path limits strictly
- Tools: assign permissions by role
