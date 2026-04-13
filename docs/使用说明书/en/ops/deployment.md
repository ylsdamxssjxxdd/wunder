---
title: Deployment and Running
summary: Before deploying Wunder, distinguish between desktop, local development, and server paths, then discuss startup commands.
read_when:
  - You need to deploy Wunder
  - You need to confirm where to place databases, MCP, sandbox, and workspaces
source_docs:
  - README.md
  - docs/API文档.md
  - docs/系统介绍.md
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
- `WUNDER_TEMP/config/wunder.yaml` (CLI/Desktop runtime)
- `config/mcp_config.json`

## Common Environment Variables

- `WUNDER_HOST`
- `WUNDER_PORT`
- `WUNDER_API_KEY`
- `WUNDER_POSTGRES_DSN`
- `WUNDER_SANDBOX_ENDPOINT`
- `WUNDER_MCP_HOST`
- `WUNDER_CONFIG_PATH`
- `WUNDER_USER_TOOLS_ROOT`

## Browser Runtime Under Docker

- Current Compose defaults to installing Playwright Chromium during image build
- Current Compose defaults to enabling `WUNDER_BROWSER_ENABLED=true`, `WUNDER_BROWSER_TOOL_ENABLED=true`, and `WUNDER_BROWSER_DOCKER_ENABLED=true`
- `shm_size: 2gb` reserves space for Chromium's `/dev/shm`, avoiding crashes, freezes, blank pages, or screenshot failures due to insufficient shared memory in the container

## Most Overlooked Deployment Issues

- Started only the server, but didn't prepare Postgres
- Enabled MCP configuration, but the target service isn't connected
- Workspaces not persisted, causing outputs to be lost
- Put long-term business data into the `config/data/` runtime directory
- Mistakenly treated desktop local mode as a server deployment method

## Further Reading

- [Server Deployment](/docs/en/start/server/)
- [Authentication and Security](/docs/en/ops/auth-and-security/)
- [Configuration Reference](/docs/en/reference/config/)