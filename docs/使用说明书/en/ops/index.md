---
title: Operations Overview
summary: Deployment, storage, security, channels, and monitoring — five main threads. The goal is to keep the system running stably over the long term.
updated_at: 2026-10-13
---

The operations section is aimed at the people responsible for deploying and maintaining the system, and covers five main threads: deployment, data and storage, accounts and security, channel management, and monitoring and model evaluation.

## Operations Entry Points

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/ops/deployment/"><strong>Deployment and Runtime</strong><span>Choose a form, prepare the environment, and run startup checks.</span></a>
  <a class="docs-card" href="/docs/en/ops/data-and-storage/"><strong>Data and Storage</strong><span>Managing data for databases, workspaces, and the knowledge base.</span></a>
  <a class="docs-card" href="/docs/en/ops/auth-and-security/"><strong>Accounts and Security</strong><span>Account systems, permission boundaries, and security settings.</span></a>
  <a class="docs-card" href="/docs/en/ops/channel-runtime/"><strong>Channel Management</strong><span>Integrating and operating external channels such as Feishu and WeChat.</span></a>
  <a class="docs-card" href="/docs/en/ops/benchmark-and-observability/"><strong>Monitoring and WunderBench</strong><span>System health, performance observation, and model evaluation.</span></a>
  <a class="docs-card" href="/docs/en/ops/wunderbench/"><strong>WunderBench Model Evaluation</strong><span>Full-suite evaluation, scoring, exports, and bank extension.</span></a>
  <a class="docs-card" href="/docs/en/ops/desktop-local-mode/"><strong>Desktop Local Mode</strong><span>The traits of local desktop runtime and how to maintain it.</span></a>
</div>

## The Five Threads

### 1. Deployment and Runtime

**Goal**: Choose the right runtime form, start it smoothly, and pass health checks.

**Key Decisions**:

- Personal use or team use
- PostgreSQL or SQLite
- Where the workspace lives

### 2. Data and Storage

**Goal**: No data loss, fast recovery, and enough storage performance.

**Storage Tiers**:

| Storage Tier | Purpose | Backup Advice |
|--------------|---------|---------------|
| Database | Users, sessions, configuration | Periodic full backup |
| Workspace | User files and outputs | Filesystem backup |
| Vector store | Knowledge base retrieval | Back up with the database |
| Temp directory | Transfer for uploads and downloads | Periodic cleanup, no backup needed |

### 3. Accounts and Security

**Goal**: Clear permissions, controlled access, and traceable audits.

**Security Points**:

- The API key is complex enough
- Path and command access is restricted
- The approval mode is configured as needed

### 4. Channel Management

**Goal**: No lost messages, stable connections, and discoverable exceptions.

**Monitoring Points**:

- Account online status
- Normal message send and receive
- Whether the outbound queue is backing up

### 5. Monitoring and WunderBench

**Goal**: Find problems early and quantify system behavior.

**Observability Focus**:

- Active session count and queue depth
- Error rate and response latency
- Token consumption trends

## Troubleshooting Priority

When the system has problems, investigate in this order:

1. **Is the entry point reachable** — first confirm the page opens and connections can be established.
2. **Is the account permission normal** — confirm the login is valid and the token has not expired.
3. **Are dependency services healthy** — confirm the database is available and MCP services are connected.
4. **Is the running state healthy** — confirm sessions are not stuck and queues are not backing up.
5. **Are metrics abnormal** — confirm the error rate and latency have not spiked.

## What to Do in Production

### Required

- [ ] Use PostgreSQL (not SQLite)
- [ ] Database automatic backup is configured
- [ ] Workspace storage is persisted (NAS/network storage)
- [ ] HTTPS certificate is configured
- [ ] The API key is complex enough

### Recommended

- [ ] The CORS policy is tightened
- [ ] Log collection is configured
- [ ] Monitoring alerts are set up
- [ ] The temp directory is cleaned up periodically

## Operations Advice by Scenario

### Personal Honeycomb Use

Little operations work is needed: update the app regularly, keep workspace files from getting too large, and use the reset feature when something goes wrong.

### Small-Team Hull (Server)

Covers database backups, user management and token issuance, and basic monitoring.

### Organizational Production Environment

Full operations are needed: a complete backup strategy, a monitoring and alerting system, security audits, channel management, and performance tuning.

## Common Misconceptions

| Symptom | Correct Understanding |
|---------|-----------------------|
| A live process means the service is available | Also confirm the entry point and dependency health |
| Treating a channel problem as a model problem | Check the integration layer first, then the model |
| Storing long-term data in the temp directory | The temp directory gets cleaned up |
| Deployment done means everything is done | Monitoring, backups, and maintenance are still needed |

## Further Reading

- [Deployment and Running](/docs/en/ops/deployment/)
- [WunderBench Model Evaluation](/docs/en/ops/wunderbench/)
- [Troubleshooting](/docs/en/help/troubleshooting/)
- [Reference Overview](/docs/en/reference/)
