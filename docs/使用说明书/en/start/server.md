---
title: Deploying the Hull (Server)
summary: An optional form. The user-and-agent management platform for multi-user sharing: multi-tenancy, permission governance, channel integration, and agent-app publishing. Once the Hull is up, the Bridge (18000) is served on the same port, and the Beehive is delivered by a separate web-service container (18001).
read_when:
  - You are deploying the Hull for a team or multiple users
  - You need its ports, config precedence, or first-login checklist
source_docs:
  - crates/wunder-server/src/main.rs
  - crates/wunder-core/src/config.rs
  - config/wunder.yaml
  - docker-compose-x86.yml
updated_at: 2026-10-13
---

# Deploying the Hull (Server)

The Hull (server) is wunder's **user-and-agent management platform** and an optional form: the Honeycomb (desktop) is enough for personal use. Deploy the Hull only when you need multi-tenancy, user and organization management, channel integration, building and publishing agent apps, and unified gateway scheduling.

## The three pieces and their ports

Once deployed, the Hull provides **three access entries at once** (default ports, all overridable by environment variables):

| Interface | Carrier | Default port |
| --- | --- | --- |
| **Hull API** | The Hull service itself | **18000** (overridable with `WUNDER_HOST` / `WUNDER_PORT`) |
| **Bridge** | The Hull's built-in native admin pages, no build needed | The same port as the Hull, 18000 |
| **Beehive** | A standalone frontend app, built and delivered by the web-service container | A separate container, **18001** (overridable with `NGINX_PORT`) |

Note: **the Beehive is not hosted by the Hull.** The Hull process only serves the Bridge pages and directories such as avatars and ppt; the Beehive is delivered by a standalone web-service container on 18001.

Other related service ports: sandbox mode 9001, onlyoffice 18080, drawio 18081, PostgreSQL 5432, extra-mcp 18004.

## When you need the Hull

| Scenario | Choose the Hull |
| --- | --- |
| Several people sharing one system | ✅ |
| Need an admin backend (the Bridge) | ✅ |
| Need to connect channels such as Feishu, WeCom, and QQ Bot | ✅ |
| Need unified governance, auditing, and quotas | ✅ |
| Need to build and publish agent apps | ✅ |
| Just personal use | ❌ Choose the Honeycomb (desktop) |

## Prerequisites

- Docker and Docker Compose (recommended)
- At least 4 GB of available memory
- PostgreSQL (built into the compose setup; a single machine can also switch to SQLite)
- Time zone: `Asia/Shanghai` by default in the compose setup

## Docker Compose deployment

```bash
# Get the code
git clone <repo-url>
cd wunder

# x86 architecture
docker compose -f docker-compose-x86.yml up -d

# ARM architecture
docker compose -f docker-compose-arm.yml up -d
```

This starts the Hull service (18000 by default), the sandbox (9001, with `/health`), onlyoffice (18080), drawio (18081), extra-mcp, PostgreSQL (5432), the Beehive frontend build container, and the web-service container (18001).

## Running locally (cargo)

```bash
# 1. Build the Beehive frontend
npm run build:frontend

# 2. Start the Hull (reads config/wunder.yaml by default, or the path given by WUNDER_CONFIG_PATH)
cargo run -p wunder-server

# Override the port
WUNDER_HOST=127.0.0.1 WUNDER_PORT=18000 cargo run -p wunder-server
```

Configuration load order: first the file named by `WUNDER_CONFIG_PATH`, else `config/wunder.yaml`; `${ENV}` placeholders in the YAML are expanded; environment variables have the highest priority and take effect last; missing fields are filled in from the example template. Note: the Rust process **does not load `.env`** — `.env` is only consumed by the compose and script layers; set real environment variables for the Hull process itself.

Known issue: starting directly with `config/wunder-example.yaml` fails to parse and silently falls back to the default configuration — point explicitly at `config/wunder.yaml`, or name it with `WUNDER_CONFIG_PATH`.

## Access entries

| Entry | Address |
| --- | --- |
| Bridge (admin interface) | `http://<host>:18000/` |
| API | `http://<host>:18000/wunder/*` |
| Beehive (user interface) | `http://<host>:18001/` |
| MCP server | `http://<host>:18000/wunder/mcp` |
| External workflows | `http://<host>:18000/wunder/external/workflow*` |
| Health check | `GET http://<host>:18000/wunder/i18n` |

## First login

The admin credentials are in `config/wunder.yaml`; change the admin password immediately after first login.

Passwords are stored with Argon2 encryption; login tokens are opaque random strings (in the form `wund_<uuid>`, `wundr_<uuid>`, `wundc_<uuid>`, **not JWTs**); refresh tokens rotate by family, and replaying the same refresh token revokes the whole family.

## Must-do after deployment

1. **Change the admin password**
2. **Bridge → Model configuration**: add LLM / TTS / image / video model endpoints and API keys
3. **Bridge → Organization management**: confirm the organization tree (up to 4 levels deep), importing `config/org_units.json` if needed
4. **Bridge → Preset agents**: create preset agent card templates that declare which fields users may customize
5. **Bridge → User management**: add users or open self-service registration
6. **For channel integration**: Feishu / WeCom / WeChat / QQ Bot / WhatsApp Cloud / weixin / xmpp (7 provided by default)
7. **Watch health**: the Bridge's "internal status" page, and the `/wunder/admin/monitor` interface

## Core capabilities

### Multi-tenancy and governance

- Users and organizations managed in layers; permissions assigned by role (admin / leader / user), with data isolated by user and agent
- Online status shown in three columns: Beehive / Honeycomb / Helm
- User levels and quotas (up to 200 levels)

### Channel integration

- 7 registered by default: WhatsApp Cloud / Feishu / QQ Bot / WeChat / WeChat Official Account / weixin / xmpp
- Of these, Feishu / QQ Bot / xmpp / weixin use long-lived connections; the rest use webhooks only
- **Telegram / Discord / Slack / LINE / DingTalk are not supported**; they exist only as generic webhook entries in the directory
- Inbound queue capacity 512, concurrency 8, enqueue timeout 80ms; outbound messages are persisted and retried with exponential backoff

### Observability

- Logs go to both the console and a JSONL file (rolled daily), written to `./config/data/logs/server` by default, at `info` level by default, and never cleaned up by default (retention 0)
- Runtime metrics: blocking, queueing, long tasks, and loop cadence, plus alerts and thresholds
- Channel run logs: the most recent 300 entries kept in memory, with duplicate floods within 20 seconds deduplicated

### Security and isolation

- Read / write / execute tools in the cloud **all run inside the sandbox**; with no available sandbox endpoint they fail outright and **do not fall back to the host**. One container per user, mounting only that user's directory, with tenant isolation handled by the sandbox.
- Exec policy has three states: allow / audit / enforce
- Approval modes: suggest / auto_edit / full_auto, with full_auto the default
- High-risk command list: rm, rm -rf, mkfs, dd, chmod 777
- Sandbox container resources: 8 CPU cores, 8096MB memory, 256 processes, 300s timeout

## Configuration files

The `config/` directory holds the engine's main `wunder.yaml`, an example configuration, sandbox runtime configuration, MCP configuration, organization units, i18n copy, version numbers, and font and drawing configuration, among others; there are also the `benchmark/`, `data/`, `fonts/`, `ppt_templates/`, `preset_worker_cards/`, `prompts/`, and `skills/` subdirectories.

Configuration supports hot reloading at runtime, and changes are written straight back to the YAML file.

## Further reading

- [Bridge interface](/docs/en/surfaces/web-admin/)
- [Beehive interface](/docs/en/surfaces/frontend/)
- [Configuration reference](/docs/en/reference/config/)
- [First-run troubleshooting](/docs/en/start/troubleshooting-first-run/)
