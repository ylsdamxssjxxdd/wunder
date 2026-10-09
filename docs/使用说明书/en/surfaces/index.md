---
title: Interface Overview
summary: Users work in the Honeycomb (local AI workbench) or the Beehive (cloud AI workbench); admins govern with the Bridge.
---

# Interface Overview

wunder's interfaces fall into two groups: **workbenches** (for users) and the **Bridge** (for admins).

## Two workbenches

Users access wunder through two workbenches, chosen by scenario:

| Workbench | Form | Positioning | Best for |
|------|------|------|------|
| **Honeycomb (desktop)** | Desktop app | Local AI workbench | Individual users; runs locally with direct access to local files and the desktop, out of the box |
| **Beehive (web)** | Web app | Cloud AI workbench | Team members; accessed in a browser, shared by many, governed by the Hull |

Both workbenches cover the same interface and capabilities: chat, the working directory, agent settings, tools, and settings. The Honeycomb adds local file access, a local runtime, and one-click reset on top of the Beehive. Individual users default to the Honeycomb; after a team deploys the Hull, members open the Beehive in a browser.

## The Bridge: governance backend

The Bridge is for administrators. It is the management UI of the Hull (the user and agent management platform), handling system configuration, user management, and channel monitoring, fully separated from the user-facing workbenches.

## Interface entries

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/surfaces/frontend/"><strong>Beehive Interface</strong><span>Two-column layout, chat, working directory, settings.</span></a>
  <a class="docs-card" href="/docs/en/surfaces/desktop-ui/"><strong>Honeycomb Interface</strong><span>Local capabilities and reset unique to the Honeycomb.</span></a>
  <a class="docs-card" href="/docs/en/surfaces/web-admin/"><strong>Bridge Interface</strong><span>User management, system configuration, channel monitoring.</span></a>
</div>

## The two-column layout

Both the Honeycomb and the Beehive use a two-column layout:

```
┌──────────────┬─────────────────────────────┐
│ Left column  │ Right column                │
│ New task/    │ Chat: top bar + timeline +  │
│ Search       │ input area                  │
│ Workspace &  │ Welcome page when no thread │
│ thread tree  │ is selected                 │
│ Working dir. │                             │
│ Settings     │                             │
└──────────────┴─────────────────────────────┘
```

The left column is fixed at 240px, top to bottom: new task, search, the workspace and thread tree, the **working directory** (file tree + toolbar + usage bar), and the settings entry at the bottom. The right column adapts, showing the welcome page or the chat page of the current thread; file preview and inline editing open as overlays without changing the two-column structure.

Every user has exactly one agent instance and one fixed workspace, so the left column holds only that single workspace and thread tree — no multi-agent list, create, or switch entry, and no way to change the directory.

## Choose by scenario

### Individual users

Install the [Honeycomb](/docs/en/start/desktop/) and work in your local AI workbench.

### Team members

After the Hull is deployed, open the [Beehive](/docs/en/surfaces/frontend/) (the cloud AI workbench) in a browser.

### Administrators

Mainly use:
- The [Bridge](/docs/en/surfaces/web-admin/) (governance — the management UI of the user and agent management platform)
- The Beehive (daily work)

## Common pitfalls

- **Can I chat from the Bridge?** No. The Bridge is a governance backend, not for daily conversation.
- **Can the Honeycomb be shared by many?** The Honeycomb is a local AI workbench for a single local user; for team collaboration deploy the Hull and use the Beehive.
- **Do the Beehive and the Bridge share one UI?** No. Different responsibilities, separate interfaces.

## Further reading

- [Beehive Interface](/docs/en/surfaces/frontend/)
- [Honeycomb Installation](/docs/en/start/desktop/)
- [Core Concepts](/docs/en/concepts/)
