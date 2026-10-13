---
title: Authentication and Security
summary: Tell the account types and permission levels apart, set clear security boundaries, and keep the system running within controlled limits.
updated_at: 2026-10-13
---

Account and permission problems fall into three categories: a difference between the identity types themselves, a tool running outside its bounds, or cross-user access. Everyday configuration covers three areas: limiting which paths and commands the agent may act on, routing sensitive operations through approval, and keeping each user's data isolated inside their own directory.

## Account Types

wunder has several identity types, each serving a different purpose.

### Administrator Accounts

- Hold system governance permissions
- Can manage users and configure the system
- Are not subject to quota or balance limits
- Own the configuration of security policies

### Regular User Accounts

- Use the system day to day
- Are subject to quota and balance limits
- Can reach only their own data
- Each has a single agent instance and a single fixed cloud working directory, which cannot be added to or replaced

### External Identities

These are identities mapped in from external platforms such as Feishu and WeCom. They are not necessarily registered users, but they still get their own isolated space.

## Layered Security

Security is layered, tightening from top to bottom:

```
┌─────────────────────────────────────┐
│   Operation security: tool approval │
│   and permission control            │
├─────────────────────────────────────┤
│   Interface security: authentication│
│   rate limiting, and CORS           │
├─────────────────────────────────────┤
│   Transport security: HTTPS and     │
│   encrypted communication           │
├─────────────────────────────────────┤
│   Network security: network         │
│   isolation and sandboxing          │
└─────────────────────────────────────┘
```

## Tool Execution Security

### Path Control

Admins can configure:

- **Allowed paths**: the directories the agent may work in
- **Blocked patterns**: file types or paths the agent may not touch

A few principles apply:

- Don't expose the whole filesystem without limits
- Name the allowed directories explicitly
- Block access to sensitive system directories

### Command Control

- Control which commands the agent may run
- High-risk commands require approval
- Admins can set an allowlist or a denylist

### Approval

Sensitive operations need the user's confirmation, including editing important files, running sensitive commands, and reaching restricted paths. How strict the approval mode is can be configured.

## Data Isolation

### User-Level Isolation

- Workspace: each user can reach only their own files
- Sessions: each user sees only their own conversations
- Configuration: personal settings don't affect anyone else

### Working-Directory Isolation

Each user has a single fixed cloud directory, and path checks are bounded by that user's root: reads and writes that step outside are rejected, and one user cannot reach another. Sub-agents share the same user directory as the main agent, so there is no further layer split by agent or container.

### Database Isolation

Every data access passes a permission check, which prevents cross-user reads.

## User Registration and Batch Provisioning

Admins can toggle **Allow user registration** in Bridge's system settings. Once it is off, the sign-in page no longer shows a registration entry, and the registration endpoint rejects new self-service sign-ups; admins can still add accounts from user management.

On Bridge's user management page, click **Add User** and pick an Excel file to import accounts in bulk. The first row must include at least `username` and `password`, and the Chinese headers `用户名/账号` and `密码` are also accepted; optional columns include `email`, `unit_id`, `status`, and `roles`. The import creates accounts row by row, lists any failed rows in the result, and leaves accounts that were already created intact.

## Security Configuration Essentials

### API Keys

- Use keys that are complex enough, at 16 or more characters with mixed character types
- Rotate keys regularly
- Never hardcode them in code or configuration
- Never commit them to version control

### Transport Security

- Production must use HTTPS
- Configure TLS certificates and renew them automatically
- WebSockets use the wss protocol

### CORS Policy

- Allow only the domains you actually need
- Don't use the wildcard `*`
- Keep development and production separate

### Log Auditing

- Record key operations such as sign-in, configuration changes, and file operations
- Retain logs long enough
- Mask sensitive information

## Handling Security Incidents

### Leaked Keys

1. Rotate the key immediately
2. Check for abnormal access
3. Review the logs to establish the scope of impact

### Unauthorized Access

1. Check the permission configuration
2. Confirm data isolation is working
3. Reset user status if needed

### Brute-Force Attempts

1. Configure sign-in rate limiting
2. Set an account lockout policy
3. Watch for abnormal sign-in attempts

## Pre-Launch Checklist

- [ ] API keys are complex enough
- [ ] HTTPS is configured
- [ ] The CORS policy is tightened
- [ ] The tool approval mode is configured
- [ ] Path and command limits are in place
- [ ] Database access is controlled
- [ ] Log auditing is enabled
- [ ] Admin passwords are rotated regularly

## Further Reading

- [Configuration Reference](/docs/en/reference/config/)
- [Multi-User Management](/docs/en/concepts/core-multi-user-management/)
- [Admin Panels Index](/docs/en/reference/admin-panels/)
