---
title: Deployment and Running
summary: Decide on a runtime form and prepare the environment before deploying. A Hull (server) deployment needs at least a database, workspace storage, and the backend service.
updated_at: 2026-10-13
---

## Runtime Forms

The three runtime forms have different setup costs.

| Form | Best For | What You Need |
|------|----------|---------------|
| **Honeycomb (desktop)** | Personal use | Download the installer and start working |
| **Hull (server)** | Teams and organizations | Server + database + configuration |
| **Local development** | Development and debugging | A local build environment |

## Honeycomb Deployment

The simplest path:

1. Download the installer.
2. Install and start it.
3. Start using it.

No extra configuration is required. Data stays on the local machine and uses SQLite.

For detailed steps, see [Desktop Getting Started](/docs/en/start/desktop/).

## Hull Deployment

### Minimum Requirements

| Component | Description | Required |
|-----------|-------------|----------|
| Backend service | Serves external APIs and scheduling | Yes |
| PostgreSQL | Primary database | Yes |
| Workspace storage | Space for user files | Yes |
| Frontend static assets | Beehive/Bridge pages | Recommended |
| MCP service | External tool capabilities | As needed |
| Sandbox service | Isolated execution environment | As needed |
| Vector database | Knowledge base retrieval | As needed |

### Recommended Deployment Method

Docker Compose is recommended:

- All components come up in one step, with configuration kept in a single file
- Upgrades are a matter of replacing images

### Configuration to Prepare

Before deploying, prepare the service listen address and port, the API key used for access, the database connection details, the configuration file location, and the sandbox service address (when the sandbox is enabled). In a Docker environment, the server build enables MCP, host metrics, and web fetch by default.

### Network Planning

Plan the external network entry points before deploying:

| Entry Point | Purpose |
|-------------|---------|
| Main service address | Backend APIs |
| WebSocket channel | Real-time communication |
| Bridge address | Admin console |
| Docs site address | User manual |

Keeping them all under a single domain and separating them by path is recommended.

### Post-Startup Checks

After startup, check in this order:

1. Confirm the backend service is running
2. Confirm the database connection is healthy
3. Confirm a WebSocket connection can be established
4. Confirm Bridge opens
5. Confirm the user side is reachable
6. Confirm the docs site is reachable

### System Status Under Docker

The CPU, memory, process, load, and disk metrics in the admin-side system status come from the host metrics capability enabled when the server is built, and the user-side web fetch tool comes from the web fetch capability. The images published by default include both. If the build options are overridden manually in the configuration, host metrics and web fetch must be kept; otherwise system resource metrics degrade to zero values and the user-side agent tool list will not show web fetch.

The fetch service configured on the admin side is synced to web search as well. The web search entry has been removed in the current version, though, so no matter how the fetch service is configured, the user-side agent tool list will not show it.

### Browsers Under Docker

To use browser tools under Docker:

- Make sure Chromium is installed when the image is built
- Allocate enough shared memory (2 GB)
- Enable the browser-related configuration

### Sandbox Write Permissions Under Docker

The sandbox container keeps a writable root filesystem by default, so file tools can reach arbitrary paths inside the container. Sandbox read-only restrictions work at two levels: one on the wunder request side, and one controlled by Docker's container-level read-only switch. If a file tool reports a read-only file system when writing a root path (such as `/test_file.txt`), the running container is usually still using an old read-only configuration; recreating the sandbox container resolves it.

## Upgrade Strategy

- Rolling updates: replace instances one at a time without interrupting service
- Database migration: handled automatically by the system, with no manual step
- Configuration compatibility: new versions adapt to older configuration formats on their own

## Common Deployment Issues

| Problem | Cause | Solution |
|---------|-------|----------|
| Startup fails | PostgreSQL isn't ready | Make sure the database starts first |
| WebSocket won't connect | Port or proxy misconfiguration | Check the port and reverse proxy settings |
| Workspace is lost | No persistent storage | Mount a data volume |
| Temp files pile up | No cleanup strategy | Configure a periodic cleanup task |
| Bridge system status resource metrics are zero | The build options were overridden and the host metrics capability is missing | Restore the default build configuration and rebuild the service and sandbox |
| Users can't see web fetch | The build options were overridden and the web fetch capability is missing | Restore the default build configuration and rebuild the service and sandbox |
| Users can't see web search | The web search entry has been removed, independent of the fetch service configuration | This is expected; no action needed |
| Browser tools are unavailable | Chromium wasn't installed when the image was built | Check the image build configuration |
| File tools fail to write to a root path | The sandbox container still has a read-only root filesystem | Confirm the sandbox write switch is at its default setting and recreate the sandbox container |

## Further Reading

- [Server Getting Started](/docs/en/start/server/)
- [Data and Storage](/docs/en/ops/data-and-storage/)
- [Configuration Reference](/docs/en/reference/config/)
