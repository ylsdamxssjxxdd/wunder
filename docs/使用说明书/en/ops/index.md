---
title: Operations Overview
summary: Wunder operations covers deployment, storage, security, channel runtime, and observability. The goal is sustainable, stable operation -- not just "it started successfully."
read_when:
  - You are deploying or taking over a Wunder runtime environment
  - You need to quickly locate operations and governance entry points
source_docs:
  - docs/设计文档/01-系统总体设计.md
  - docs/API文档.md
---

# Operations Overview

Think of Wunder operations as **five main threads**: deployment, storage, security, channels, and observability.

The goal: keep the system **running stably for 10+ years**, not just "it started successfully."

---

## Operations Entry Points

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/ops/deployment/">
    <strong>Deployment and Runtime</strong>
    <span>Startup paths, dependencies, and health checks.</span>
  </a>
  <a class="docs-card" href="/docs/en/ops/data-and-storage/">
    <strong>Data and Storage</strong>
    <span>Postgres/SQLite/Weaviate/workspaces/temp_dir.</span>
  </a>
  <a class="docs-card" href="/docs/en/ops/auth-and-security/">
    <strong>Authentication and Security</strong>
    <span>Token system, boundaries, and approval policies.</span>
  </a>
  <a class="docs-card" href="/docs/en/ops/channel-runtime/">
    <strong>Channel Runtime</strong>
    <span>Webhooks, persistent connections, outbox, and recovery.</span>
  </a>
  <a class="docs-card" href="/docs/en/ops/benchmark-and-observability/">
    <strong>Monitoring and Benchmark</strong>
    <span>Throughput, latency, errors, and capacity assessment.</span>
  </a>
  <a class="docs-card" href="/docs/en/ops/desktop-local-mode/">
    <strong>Desktop Local Mode</strong>
    <span>Local runtime boundaries and governance essentials.</span>
  </a>
</div>

---

## The Five Operations Threads

### 1. Deployment and Runtime

**Goal**: One-click startup, health checks, graceful upgrades

**Key points**:
- Docker Compose is the recommended deployment method
- Separate build, dependencies, and runtime stages
- Health checks: frontend waits for backend, Nginx waits for frontend
- Upgrade strategy: rolling updates, automatic data migration

**Checklist**:
- [ ] Docker and Docker Compose versions are recent enough
- [ ] No port conflicts (18000, 18002; also check 18001 if exposing the frontend dev server directly)
- [ ] Data volume mounts are correct
- [ ] Health checks pass

---

### 2. Data and Storage

**Goal**: No data loss, fast recovery, sufficient performance

**Storage tiers**:

| Tier | Technology | Purpose | Backup Strategy |
|------|-----------|---------|-----------------|
| **Relational data** | PostgreSQL | Users, sessions, configuration | Periodic full backup + binlog |
| **Vector data** | Weaviate | Knowledge base retrieval | Backup alongside PostgreSQL |
| **File storage** | Local disk/NAS | User workspaces | Filesystem backup |
| **Temporary data** | Local disk | Uploads, downloads, conversions | Periodic cleanup |

**Important reminders**:
- Do NOT mix long-lived business data into the `config/data/` runtime directory
- Do NOT treat `temp_dir` as a business repository
- DO persist user workspaces
- DO have a backup strategy for PostgreSQL

---

### 3. Authentication and Security

**Goal**: Prevent unauthorized access, maintain audit trails, ensure data security

**Security layers**:

```
┌─────────────────────────────────────┐
│   Application: tool approval,       │
│   permission control                │
│   - approval_mode                  │
│   - tool allowlist/blocklist        │
└─────────────────────────────────────┘
              ↑
┌─────────────────────────────────────┐
│   API layer: auth, CORS, rate limit │
│   - API Key / Bearer Token         │
│   - CORS policy                     │
│   - Rate limiting                   │
└─────────────────────────────────────┘
              ↑
┌─────────────────────────────────────┐
│   Network: HTTPS, reverse proxy     │
│   - Nginx TLS termination          │
│   - Origin validation               │
└─────────────────────────────────────┘
              ↑
┌─────────────────────────────────────┐
│   Infrastructure: network isolation,│
│   container security                │
│   - Network policies                │
│   - Container read-only root FS     │
└─────────────────────────────────────┘
```

**Key configuration**:
- `security.api_key`: Admin API Key (make it complex!)
- `security.cors.allowed_origins`: Tighten CORS policy
- `tools.builtin.enabled`: Control built-in tool visibility
- `approval_mode`: Tool approval level

---

### 4. Channel Runtime

**Goal**: No message loss, automatic reconnection, observability

**Channel architecture**:

```
External channels (Feishu/WeChat/QQ/XMPP)
        ↓ Webhook / Persistent connection
   ChannelHub (unified entry point)
        ↓
   Inbound queue (fast acknowledgment)
        ↓
   Dispatch execution (async processing)
        ↓
   channel_outbox (outbound queue)
        ↓
   Channel adapter (delivery + retry)
```

**Monitoring essentials**:
- Inbound success/failure/retry counts
- Outbound success/failure/retry counts
- Persistent connection status (online/offline)
- Outbox backlog depth

**Recovery mechanisms**:
- Webhook: Idempotent processing + retries
- Persistent connections: Auto-reconnect + heartbeat
- Outbox: Persistence + background worker

---

### 5. Monitoring and Benchmark

**Goal**: Detect problems early, plan capacity, tune performance

**Key metrics**:

| Metric | Description | Alert Threshold |
|--------|-------------|-----------------|
| **Active sessions** | Currently executing sessions | > max_active_sessions * 0.8 |
| **Queue depth** | Pending agent_tasks | > 100 |
| **Error rate** | Failed requests / total requests | > 5% |
| **Average latency** | End-to-end response time | > 30s |
| **Token consumption** | Context usage, check `round_usage.total_tokens` first | Approaching max_context |

**Built-in capabilities**:
- Throughput stress-test endpoint
- Performance sampling endpoint
- PinchBench capability evaluation
- Session monitoring dashboard

---

## Troubleshooting Priority

When something goes wrong, investigate in this order:

1. **Is the entry point reachable?**
   - HTTP status code?
   - Can WebSocket connect?
   - DNS resolution normal?

2. **Is authentication correct?**
   - Is the API Key correct?
   - Has the Token expired?
   - Any CORS errors?

3. **Are dependencies ready?**
   - PostgreSQL healthy?
   - Weaviate healthy?
   - Sandbox healthy?
   - MCP services healthy?

4. **Is the runtime healthy?**
   - Session runtime state?
   - Channel connection state?
   - Outbox backlog?

5. **Are metrics abnormal?**
   - Error rate spike?
   - Latency spike?
   - Queue depth spike?

---

## Production Environment Checklist

- [ ] Using PostgreSQL instead of SQLite
- [ ] Database automatic backup configured
- [ ] Workspace storage persisted (NAS/network storage)
- [ ] HTTPS certificate configured with auto-renewal
- [ ] CORS policy tightened to necessary domains only
- [ ] API Key is sufficiently complex (>16 chars, mixed characters)
- [ ] Log collection configured (ELK/Loki, etc.)
- [ ] Monitoring alerts set up (Prometheus/Grafana, etc.)
- [ ] Resource limits configured (Docker cgroups)
- [ ] Channel outbox worker enabled
- [ ] Periodic cleanup strategy for temp_dir

---

## Common Misconceptions

| Misconception | Correct Understanding |
|---------------|-----------------------|
| Process alive = service available | Must check entry point and dependency health |
| Channel problem = model problem | Check the integration layer first, then the model |
| temp_dir can store long-term data | temp_dir is temporary storage and will be cleaned up |
| Deployment is done = all done | You need monitoring, backups, and upgrades |

---

## Next Steps

- Having issues? See [Troubleshooting](/docs/en/help/troubleshooting/)
- Looking for references? See [Reference Overview](/docs/en/reference/)
- Have questions? See [FAQ](/docs/en/help/faq/)
