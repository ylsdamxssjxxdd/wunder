---
title: Deployment and Running
summary: Before deploying Wunder, distinguish between desktop, local development, and server paths, then discuss startup commands.
read_when:
  - You need to deploy Wunder
  - You need to confirm where to place databases, MCP, sandbox, and workspaces
source_docs:
  - README.md
  - docs/API文档.md
  - docs/设计文档/01-系统总体设计.md
  - config/wunder-example.yaml
---

# Deployment and Running

Before deploying Wunder, don't rush to ask "how to start"—first ask "which runtime form do I need?"

## Key Points on This Page

- How to choose the right deployment path first
- What minimum components to prepare on the server side
- Which endpoints to check first after startup

## Decide These Four Things First

- Will you use it locally as an individual, or deploy it for a team online?
- Should you use PostgreSQL or SQLite for the database?
- Where will workspaces be located, and should they be persisted?
- Should sandbox, MCP, and static frontend be deployed together?

## Choose Deployment Path by Goal

### Desktop

- Suitable for direct personal use
- Local-first
- Comes with desktop shell
- Does not require building a complete server first

### Server

- Suitable for teams, organizations, and unified governance
- Multi-user, multi-tenant
- Admin and user sides working together
- Can connect to sandbox, MCP, A2A, and external channels

### Local Development

- Suitable for developer integration testing
- Rust backend and frontend dev server debugging in layers
- Can start only one layer for partial verification

## If You Deploy Server, What's the Minimum Required

In a typical deployment, at minimum these components are involved:

- `wunder-server`
- PostgreSQL
- Persistent user workspaces

If you need more complete capabilities, connect as needed:

- `wunder-sandbox`
- `extra-mcp`
- User-side or admin-side static resource services

## How to Plan External Paths

- `/wunder`
- `/wunder/chat/*`
- `/a2a`
- `/.well-known/agent-card.json`
- `/docs/`

These endpoints can all be mounted under the same service domain—don't wait until just before going live to piece them together.

## Check These First After Startup

1. Can `/wunder` return a response?
2. Can `/wunder/chat/ws` establish a connection?
3. Can `/a2a/agentCard` be read?
4. Is `/wunder/mcp` reachable?
5. Can `/docs/` be opened normally?

## Key Configuration Files

- `config/wunder.yaml`
- `config/wunder-example.yaml`
- `~/.wunder/cli/WUNDER_TEMP/config/wunder.yaml` (CLI runtime)
- `config/mcp_config.json`

## Common Environment Variables

- `WUNDER_HOST`
- `WUNDER_PORT`
- `WUNDER_API_KEY`
- `WUNDER_POSTGRES_DSN`
- `WUNDER_SANDBOX_ENDPOINT`
- `WUNDER_SANDBOX_DOCKER_READ_ONLY`
- `WUNDER_SERVER_FEATURES`
- `WUNDER_MCP_HOST`
- `WUNDER_CONFIG_PATH`
- `WUNDER_USER_TOOLS_ROOT`

## System Status Under Docker

The admin-side system status CPU, memory, process, load, and disk metrics depend on the `host-metrics` Rust feature. Compose now defaults to `WUNDER_SERVER_FEATURES=mcp,host-metrics`; if you override this variable in `.env`, keep `host-metrics` or those host resource metrics will degrade to zero values.

## Browser Runtime Under Docker

- Current Compose defaults to installing Playwright Chromium during image build
- Current Compose defaults to enabling `WUNDER_BROWSER_ENABLED=true`, `WUNDER_BROWSER_TOOL_ENABLED=true`, and `WUNDER_BROWSER_DOCKER_ENABLED=true`
- `shm_size: 2gb` reserves space for Chromium's `/dev/shm`, avoiding crashes, freezes, blank pages, or screenshot failures due to insufficient shared memory in the container

## Sandbox Write Permissions Under Docker

Current Compose keeps the `wunder-sandbox` container root filesystem writable by default so file tools can write arbitrary container paths. `WUNDER_SANDBOX_READONLY_ROOTFS` controls Wunder's request-level sandbox flag; Docker Compose's container-level `read_only` flag is controlled separately by `WUNDER_SANDBOX_DOCKER_READ_ONLY`, which defaults to `false`. If a file tool writes a root path such as `/test_file.txt` and gets `Read-only file system (os error 30)`, check whether the running sandbox container was created from an older `read_only: true` config and recreate it.

## Most Overlooked Deployment Issues

- Started only the server, but didn't prepare Postgres
- Enabled MCP configuration, but the target service isn't connected
- Workspaces not persisted, causing outputs to be lost
- Put long-term business data into the `config/data/` runtime directory
- Admin system status resource metrics show zero because `WUNDER_SERVER_FEATURES` was overridden without `host-metrics`
- Sandbox file tools still fail with `Read-only file system` because an old `wunder-sandbox` container was created with Docker `read_only: true`
- Mistakenly treated desktop local mode as a server deployment method

## Further Reading

- [Server Deployment](/docs/en/start/server/)
- [Authentication and Security](/docs/en/ops/auth-and-security/)
- [Configuration Reference](/docs/en/reference/config/)
