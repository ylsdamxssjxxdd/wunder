---
title: Server Deployment
summary: The choice for teams and organizations. Multi-user, permission management, channel integration, unified governance.
read_when:
  - You need multiple people sharing wunder
  - You need an admin backend and unified management
source_docs:
  - docs/API文档.md
updated_at: 2026-04-10
---

# Server Deployment

Server is the choice for teams and organizations. Multi-user collaboration, permission management, channel integration — all require deploying Server first.

## When to Choose Server

| Scenario | Choose Server |
|----------|--------------|
| Multiple people sharing one system | ✅ |
| Need an admin backend | ✅ |
| Need to connect Feishu, WeChat, etc. | ✅ |
| Need unified governance and auditing | ✅ |
| Just for personal use | ❌ Choose Desktop |

## Prerequisites

- Docker and Docker Compose (recommended)
- At least 4GB available memory
- PostgreSQL database (included automatically with Docker deployment)

## 3 Steps to Deploy

### 1. Get the Code

```bash
git clone <repo-url>
cd wunder
```

### 2. Start the Service

```bash
# x86 architecture
docker-compose -f docker-compose-x86.yml up -d

# ARM architecture
docker-compose -f docker-compose-arm.yml up -d
```

### 3. Access the System

- User frontend: http://localhost:18002
- Admin & docs: http://localhost:18000
- Default admin: admin / admin

**Change the default password immediately after first login.**

## Post-Deployment Checklist

1. **Change default password**: The admin default password is not secure
2. **Configure models**: Admin → Model Configuration → Add API Key
3. **Create users**: Admin → User Management → Add users or enable registration
4. **Check channels**: If you need external channels, configure credentials first

## Core Capabilities

### Multi-tenancy

- Users and organizations managed in layers
- Permissions assigned by role
- Data isolation

### Channel Integration

Supported external channels:
- Feishu (Lark)
- WeCom / WeChat
- QQ Bot
- WhatsApp Cloud
- XMPP

### Observability

- Service health monitoring
- Performance metrics
- Logs and auditing

### Security

- Token-based authentication
- Sandbox isolation
- Approval policies
- Request limits

## Configuration Files

Main configuration files are in the `config/` directory:

- Model configuration
- Tool configuration
- Channel configuration
- Security policies

Changes usually require a service restart.

## Next Steps

- [Admin Interface](/docs/en/surfaces/web-admin/)
- [Authentication & Security](/docs/en/ops/auth-and-security/)
- [Deployment & Operations](/docs/en/ops/deployment/)
- [Configuration Reference](/docs/en/reference/config/)
