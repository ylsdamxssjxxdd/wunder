---
title: Server Deployment
summary: Start with `wunder-server` only when you need multi-user governance, a unified access layer, and an admin backend.
read_when:
  - You need to deploy the server runtime form
  - You need multi-user, organizational governance, and external integration capabilities
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
  - docs/API文档.md
---

# Server Deployment

If you need **team collaboration, multi-user governance, and unified external interfaces**, this is the page to read.

`wunder-server` is the core service form of wunder. It is responsible for multi-tenant governance, unified access, and the admin backend.

---

## When do you need Server?

| Scenario | Choose Server |
|------|-----------|
| Concurrent access by multiple users | ✅ |
| Governance for organizations, units, tenants, and admins | ✅ |
| Unified exposure of `/wunder`, chat interfaces, and `A2A` | ✅ |
| Using wunder as a platform capability for other systems | ✅ |
| Personal local use only | ❌ Choose Desktop |

---

## Server architecture overview

```text
                    ┌─────────────────┐
                    │ External Access │
                    │ (users / apps)  │
                    └────────┬────────┘
                             │
                    ┌────────▼────────┐
                    │  Nginx (18002)  │
                    │ reverse proxy + │
                    │ static assets   │
                    └────────┬────────┘
         ┌───────────────────┼───────────────────┐
         │                   │                   │
    ┌────▼────┐        ┌────▼────┐        ┌────▼────┐
    │Frontend │        │ wunder- │        │ Static  │
    │ (user)  │        │ server  │        │ docs    │
    └─────────┘        └────┬────┘        └─────────┘
                             │
         ┌───────────────────┼───────────────────┐
         │                   │                   │
    ┌────▼────┐        ┌────▼────┐        ┌────▼────┐
    │Postgres │        │ Weaviate│        │ wunder- │
    │ primary │        │ vector  │        │ sandbox │
    │   db    │        │  store  │        │         │
    └─────────┘        └─────────┘        └─────────┘
         │                                       │
    ┌────▼────┐                             ┌────▼────┐
    │Workspace│                             │ extra-  │
    │ storage │                             │ mcp     │
    └─────────┘                             └─────────┘
```

### What Server is responsible for

| Capability | What it covers |
|------|------|
| **Execution entry** | `/wunder` low-level agent orchestration |
| **Chat entry** | `/wunder/chat/*` complete chat interfaces |
| **A2A interface** | `/a2a` system-level agent interoperability |
| **Streaming** | WebSocket plus SSE |
| **Governance** | users, units, permissions, and token accounts |
| **Channel integration** | Feishu, WeChat, QQ, XMPP, and more |

---

## Preparation before deployment

### Hardware requirements

| Scale | CPU | Memory | Disk |
|------|-----|------|------|
| Small, under 10 users | 2 cores | 4 GB | 50 GB |
| Medium, 10 to 50 users | 4 cores | 8 GB | 100 GB |
| Large, over 50 users | 8+ cores | 16+ GB | 200+ GB |

### Software dependencies

- Docker 20.10+
- Docker Compose 2.0+
- or a direct Rust binary deployment, which still requires PostgreSQL

### Database choice

| Scenario | Database |
|------|--------|
| Production | PostgreSQL, recommended |
| Development and testing | PostgreSQL, usually from Docker Compose |
| Desktop runtime | SQLite3 |

> Important: the Server runtime **does not use** SQLite. It must use PostgreSQL.

---

## Docker Compose deployment

### 1. Get the code

```bash
git clone <repo-url>
cd wunder
```

### 2. Configure environment variables, optional

Copy `.env.example` to `.env` and adjust it as needed:

```bash
cp .env.example .env
```

Key settings:

| Key | Meaning | Default |
|--------|------|--------|
| `WUNDER_PORT` | server port | 18000 |
| `WUNDER_TEMP_DIR_ROOT` | temp file directory | ./config/data/temp_dir |
| `DATABASE_URL` | PostgreSQL connection string | postgres://... |

### 3. Start the services

**x86**
```bash
docker-compose -f docker-compose-x86.yml up -d
```

**ARM, including Apple Silicon and Raspberry Pi**
```bash
docker-compose -f docker-compose-arm.yml up -d
```

### 4. Wait for startup

On first boot, the system will:

- pull or build images
- initialize the PostgreSQL database
- start wunder-server, the frontend build service, nginx, sandbox, and related components

Wait about one to two minutes, then check status:

```bash
docker-compose -f docker-compose-x86.yml ps
```

### 5. Access the system

| Service | Address | Notes |
|------|------|------|
| User frontend | `http://localhost:18002` | Nginx-exposed entry for regular users |
| Frontend dev service | `http://localhost:18001` | Compose-internal frontend build and debugging entry |
| Admin UI / docs | `http://localhost:18000` | Rust backend serving `web/`, debug pages, and `/docs/` |
| Default admin | `admin / admin` | Change the password after the first login |

> In the default compose setup, `18002` is for end users, `18000` is for admins, debugging, and docs, and `18001` is mainly for frontend integration work rather than production traffic.

---

## Key configuration points

### Configuration file

Main config file: `config/wunder.yaml`

```yaml
server:
  mode: server
  port: 18000
  max_active_sessions: 100

llm:
  models:
    - name: gpt-4o
      api_key: your-api-key
      endpoint: https://api.openai.com/v1
      max_context: 128000
      max_rounds: 20

database:
  url: postgres://wunder:wunder@postgres:5432/wunder

security:
  api_key: your-admin-api-key
  cors:
    allowed_origins:
      - http://localhost:18002
      - http://localhost:18001
```

### Persistent data

| Data type | Docker volume or path | Notes |
|----------|-----------|------|
| User workspaces | `wunder_workspaces` | `/workspaces` |
| PostgreSQL data | `wunder_logs` | PostgreSQL data directory |
| Temp files | `./config/data/temp_dir` | temporary uploads and downloads |
| Service runtime logs | `./config/data/logs/server` | JSONL logs written only by `wunder-server`, retained for 14 days by default |

> Important: do not mix long-lived business data into the runtime directories under `config/data/`. Durable business files should live in workspaces or external storage.

### Service logs

The server runtime writes both console logs and local file logs:

- local log directory: `config/data/logs/server`
- log format: structured JSONL with daily rotation
- covered events: startup, HTTP access, abnormal exits, and panic
- not covered: desktop and cli do not use this same local runtime log system

---

## External access planning

### Paths to expose

Behind Nginx, the recommended exposed paths are:

| Path | Purpose |
|------|------|
| `/` | user frontend |
| `/wunder` | core API |
| `/wunder/chat` | chat API |
| `/wunder/admin` | admin API, must be protected |
| `/a2a` | A2A interface |
| `/.well-known/agent-card.json` | agent discovery |

### Key constraints

| Constraint | Meaning |
|------|------|
| `user_id` does not need registration | any virtual identifier is valid |
| Prefer WebSocket | use SSE as a fallback |
| Business systems should call `/wunder` | low-level execution entry |
| Stable session-style calls should use `/wunder/chat/*` | full conversation entry |

---

## Production checklist

- [ ] PostgreSQL is used instead of SQLite
- [ ] Database backup strategy is configured
- [ ] Workspace storage is persistent
- [ ] HTTPS certificates are configured
- [ ] CORS policy is tightened
- [ ] API keys are sufficiently strong
- [ ] Log collection is configured
- [ ] Monitoring and alerting are configured
- [ ] Resource limits are configured

---

## Common questions

**Q: Can it work together with Desktop?**  
A: Yes. Desktop can connect to Server as a remote gateway.

**Q: Does `user_id` need to be registered first?**  
A: No. External callers can pass any virtual `user_id`.

**Q: Is exposing only `/wunder` enough?**  
A: It works, but planning chat endpoints and WebSocket support gives a better experience.

**Q: How do upgrades work?**  
A: Pull the latest code and run `docker-compose up -d` again.

---

## Next

- Need more deployment detail? -> [Deployment and Operations](/docs/en/ops/deployment/)
- Need security guidance? -> [Authentication and Security](/docs/en/ops/auth-and-security/)
- Want the system architecture? -> [Architecture](/docs/en/concepts/architecture/)
- Need the API entry point? -> [wunder API](/docs/en/integration/wunder-api/)
