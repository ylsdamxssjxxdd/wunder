---
title: Configuration
summary: When configuration doesn't take effect, first confirm which layer you're modifying, rather than suspecting the program isn't reading it.
read_when:
  - You are checking where a specific configuration should be written
  - You need to distinguish between base configuration, runtime overrides, and external MCP configuration
source_docs:
  - config/wunder-example.yaml
  - docs/API文档.md
  - docs/系统介绍.md
---

# Configuration

Wunder's configuration is not handled by a single file, but is organized in layers.

Key takeaway: When troubleshooting configuration issues, first distinguish between "base configuration, runtime overrides, and external MCP configuration" layers.

## Key Points on This Page

- Where specific configurations should be modified
- Which configurations are read by server vs extra_mcp
- Why your configuration changes may not have taken effect

## Key Configuration Files

- `config/wunder.yaml`
- `config/wunder-example.yaml`
- `WUNDER_TEMP/config/wunder.yaml` (CLI/Desktop runtime copy)
- `config/mcp_config.json`

## What Each File Is Responsible For

### `config/wunder.yaml`

- Official base configuration
- Usually the first place to look when deploying server

### `config/wunder-example.yaml`

- Example configuration and fallback template
- When the official configuration is missing, the system falls back to this file according to current logic

### `WUNDER_TEMP/config/wunder.yaml`

- The configuration file actually used by CLI/Desktop at runtime
- Usually corresponds to the content saved from the admin panel
- Not recommended to mix manual edits with base configuration

### `config/mcp_config.json`

- Independent MCP service configuration
- Especially for database and knowledge base related tools

## Find Configuration by Problem

- For service listening and concurrency limits, see `server.*`
- For authentication, commands, and path control, see `security.*`
- For external MCP service integration, see `mcp.*`
- For A2A service integration, see `a2a.*`
- For storage and vector capabilities, see `storage.*` and `vector_store.*`
- For browser, only need to check `tools.browser.enabled`, `browser.enabled`, and `browser.docker.enabled`; other parameters use system defaults

## Items You're Most Likely to Modify First

- Service basics: `server.host`, `server.port`, `server.chat_stream_channel`, `server.max_active_sessions`
- Security controls: `security.api_key`, `security.external_auth_key`, `security.allow_commands`, `security.allow_paths`, `security.deny_globs`
  - Note: Setting `security.allow_paths` to `*` opens the entire filesystem; if `deny_globs` is also present, matching paths will still be blocked.
- MCP: `mcp.timeout_s`, `mcp.servers[]`
- A2A: `a2a.timeout_s`, `a2a.services[]`

## Three Things to Check When Configuration Doesn't Take Effect

1. Whether you modified the repository configuration or the runtime copy
2. Which path the currently running instance actually reads from
3. Whether this configuration is consumed by server, desktop, or extra_mcp

## Common Misconceptions

- Treating `wunder-example.yaml` as the official configuration for long-term modifications.
- Modifying both repository `config/wunder.yaml` and runtime copy `WUNDER_TEMP/config/wunder.yaml` simultaneously; first confirm which file the current instance actually uses.
- Assuming all configurations are read by the server process; in reality, `extra_mcp` has its own configuration file.

## Further Reading

- [Deployment and Running](/docs/en/ops/deployment/)
- [MCP Endpoint](/docs/en/integration/mcp-endpoint/)
- [A2A Interface](/docs/en/integration/a2a/)