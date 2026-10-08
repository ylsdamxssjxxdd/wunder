---
title: Admin Interface
summary: The admin's backend workbench. Model configuration, tool management, user governance, channel integration — all here.
---

# Admin Interface

The admin interface is the system administrator's backend workbench. Regular users don't see this interface.

## When You Need the Admin Interface

- Need to configure models and API Keys
- Need to manage users, organizations, permissions
- Need to connect external channels (Feishu, WeChat, etc.)
- Need to manage tools and skills
- Need to view system running status

## Core Features

### Model Configuration

- Add, edit, delete models
- Configure API Keys and endpoints
- Test model connections
- Set default models

### Preset Agents

- Manage preset agent templates (there can be many templates; each user binds exactly one)
- Configure avatar, name, description, model, tools, prompts, and built-in skills
- Declare per field whether the user may customize it (system prompt, welcome, default model, reasoning effort, tool set, approval mode)
- A "Bound users" section: list users bound to the preset, and batch bind / rebind / unbind (unbinding requires a new preset)
- Sync to bound users' single instance: "sync uncustomized fields" only touches fields the user has not changed, "force overwrite" covers all fields; preview the affected user count first, and never rewrite the system prompt of frozen threads
- The preset form no longer carries swarm fields or a container ID field

### Tool Management

- View all available tools
- Enable / disable tools
- Manage skill files
- Configure MCP / A2A services

### Users & Organizations

- User list and status management
- Organization management
- Permission and role assignment
- Token management

### Channel Management

- Configure Feishu, WeCom, QQ, and other channels
- Manage channel credentials
- View channel running status

### System Monitoring

- Service health status
- Performance metrics
- Log viewing

## Access

The admin interface is typically accessed via browser after Server deployment, default port 18000.

## Further Reading

- [Server Deployment](/docs/en/start/server/)
- [Authentication & Security](/docs/en/ops/auth-and-security/)
- [Configuration Reference](/docs/en/reference/config/)
