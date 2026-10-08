---
title: Interface Overview
summary: Users work in Beehive; admins govern in the admin interface. Beehive opens via the desktop app or a web browser.
---

# Interface Overview

wunder's interfaces fall into two categories: **Beehive** is every user's work surface, and **the admin interface** is the administrator's governance backend.

## Beehive: user work surface

Beehive is the daily work surface for all users, covering chat, the working directory, agent settings, tools, and settings. Individual users install the desktop app; team members open Beehive in a web browser managed by the server. Both share the same interface and capabilities, and the desktop app adds local file access, a local runtime, and one-click reset on top of Beehive.

## Admin interface: governance backend

The admin interface is for administrators — system configuration, user management, channel monitoring. It serves a completely different purpose from daily user work, so it's a separate surface and doesn't do daily chat.

## Surface entries

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/surfaces/frontend/"><strong>Beehive Interface</strong><span>Two-column layout, chat, working directory, settings.</span></a>
  <a class="docs-card" href="/docs/en/surfaces/desktop-ui/"><strong>Desktop Interface</strong><span>Desktop-only local capabilities and reset.</span></a>
  <a class="docs-card" href="/docs/en/surfaces/web-admin/"><strong>Admin Interface</strong><span>User management, system config, channel monitoring.</span></a>
</div>

## Beehive's two-column layout

Both the desktop app and the web browser use a two-column layout:

```
┌──────────────┬─────────────────────────────┐
│ Left         │ Right                       │
│ New/Search   │ Chat: header + timeline +   │
│ Workspace    │ composer; welcome page when │
│ thread tree  │ no thread is selected       │
│ Working dir  │                             │
│ Settings     │                             │
└──────────────┴─────────────────────────────┘
```

The left sidebar is fixed at 240px and stacks, top to bottom: new task, search, the workspace and thread tree, the **working directory** (file tree + toolbar + usage bar), and the settings entry at the bottom. The right column is fluid and shows either the welcome page or the current thread's chat; file preview and online editing open as overlays without changing the two-column structure.

Every user has exactly one agent instance and one fixed cloud working directory, so the sidebar holds only that single workspace and thread tree — no multi-agent list, create, or switch entry, and no way to change the directory.

## Pick by role

### Regular users

Mainly use Beehive:
- Individual users install the [desktop app](/docs/en/start/desktop/)
- Team members open Beehive in a browser

### Administrators

Mainly use:
- [Admin interface](/docs/en/surfaces/web-admin/) (for governance)
- Beehive (for daily work)

## Common misconceptions

- **Can the admin interface chat?** No. It's a governance backend, not for daily chat.
- **Can the desktop app switch to a remote server?** The current version focuses on local mode. For team capabilities, use the web browser.
- **Do Beehive and the admin interface share one interface?** No. Different responsibilities, separate surfaces.

## Further reading

- [Beehive Interface](/docs/en/surfaces/frontend/)
- [Desktop Guide](/docs/en/start/desktop/)
- [Core Concepts](/docs/en/concepts/)
