---
title: Interface Overview
summary: Users work in Hive; admins govern in the admin interface. Hive opens via the desktop app or a web browser.
---

# Interface Overview

wunder's interfaces fall into two categories: **Hive** is every user's work surface, and **the admin interface** is the administrator's governance backend.

## Hive: user work surface

Hive is the daily work surface for all users, covering chat, files, agents, tools, and settings. It has two ways to open:

| Way | For | Notes |
|------|------|------|
| **Desktop app** | Individual users | Local install, can access local files and desktop, runs out of the box |
| **Web browser** | Team members | Browser access, multi-user, managed by the server |

Both share the same interface and capabilities. The desktop app adds local file access, a local runtime, and one-click reset on top of Hive.

## Admin interface: governance backend

The admin interface is for administrators — system configuration, user management, channel monitoring. It serves a completely different purpose from daily user work, so it's a separate surface and doesn't do daily chat.

## Surface entries

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/surfaces/frontend/"><strong>Hive Interface</strong><span>Chat, files, agents, tools, settings.</span></a>
  <a class="docs-card" href="/docs/en/surfaces/orchestration/"><strong>Orchestration</strong><span>The dedicated workspace for queen-worker-artifact flow and timelines.</span></a>
  <a class="docs-card" href="/docs/en/surfaces/desktop-ui/"><strong>Desktop Interface</strong><span>Desktop-only local capabilities and reset.</span></a>
  <a class="docs-card" href="/docs/en/surfaces/web-admin/"><strong>Admin Interface</strong><span>User management, system config, channel monitoring.</span></a>
</div>

## Hive's three-column layout

Both the desktop app and the web browser use a three-column layout:

```
┌────────┬─────────────┬──────────────────┐
│ Left   │ Middle      │ Right            │
│ Nav    │ List        │ Workspace        │
│        │ Sessions/   │ Chat / details   │
│        │ Files       │                  │
└────────┴─────────────┴──────────────────┘
```

- **Left column**: top-level navigation (Chat, Files, Agents, Tools, Settings, etc.)
- **Middle column**: lists (session list, file list, agent list, etc.)
- **Right column**: workspace (chat detail, file preview, settings panel, etc.)

## Pick by role

### Regular users

Mainly use Hive:
- Individual users install the [desktop app](/docs/en/start/desktop/)
- Team members open Hive in a browser

### Administrators

Mainly use:
- [Admin interface](/docs/en/surfaces/web-admin/) (for governance)
- Hive (for daily work)

## Common misconceptions

- **Can the admin interface chat?** No. It's a governance backend, not for daily chat.
- **Can the desktop app switch to a remote server?** The current version focuses on local mode. For team capabilities, use the web browser.
- **Do Hive and the admin interface share one interface?** No. Different responsibilities, separate surfaces.

## Further reading

- [Hive Interface](/docs/en/surfaces/frontend/)
- [Desktop Guide](/docs/en/start/desktop/)
- [Orchestration](/docs/en/surfaces/orchestration/)
- [Core Concepts](/docs/en/concepts/)
