---
title: Interface Overview
summary: Wunder currently maintains three interface surfaces — the user-side frontend, the admin frontend, and the desktop client. They share the same backend capabilities but serve different responsibilities.
read_when:
  - You want to quickly understand what the user frontend, admin frontend, and desktop each do
  - You are looking for which interface a particular feature belongs to
source_docs:
  - docs/设计文档/01-系统总体设计.md
  - docs/API文档.md
---

# Interface Overview

Wunder is not a single-page chat product. It is three interface surfaces working together.

If you are trying to figure out "where should I look for a particular feature," start here.

## Quick Entry

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/surfaces/frontend/">
    <strong>User Frontend</strong>
    <span>The unified workbench for chat, user world, workspace, tools, and settings.</span>
  </a>
  <a class="docs-card" href="/docs/en/surfaces/orchestration/">
    <strong>Orchestration</strong>
    <span>The dedicated workbench for queen-worker-artifact flow, timeline snapshots, and branching.</span>
  </a>
  <a class="docs-card" href="/docs/en/surfaces/web-admin/">
    <strong>Admin Interface</strong>
    <span>The management entry point for models, users, channels, tools, monitoring, and benchmarks.</span>
  </a>
  <a class="docs-card" href="/docs/en/surfaces/desktop-ui/">
    <strong>Desktop Interface</strong>
    <span>The primary local delivery form, emphasizing local-first and desktop workbench experience.</span>
  </a>
</div>

## Responsibilities of Each Surface

- **User frontend**: Focused on conversations, files, contacts, and tool operations
- **Orchestration**: Focused on long-running hive execution, round snapshots, artifacts, and branching
- **Admin frontend**: Focused on governance, configuration, monitoring, and evaluation
- **Desktop**: Focused on local-first and desktop capabilities

## By Role

### Regular Users

Start with:

- [User Frontend](/docs/en/surfaces/frontend/)
- [Orchestration](/docs/en/surfaces/orchestration/)
- [Desktop Interface](/docs/en/surfaces/desktop-ui/)

### Administrators

Start with:

- [Admin Interface](/docs/en/surfaces/web-admin/)
- [Admin Panel Index](/docs/en/reference/admin-panels/)

### Integration Developers

In addition to the interface pages, you should also read:

- [Chat Sessions](/docs/en/integration/chat-sessions/)
- [Workspace API](/docs/en/integration/workspace-api/)

## Common Misconceptions

- The user frontend emphasizes conversation experience and the file-agent loop.
- The admin frontend emphasizes governance, monitoring, configuration, and evaluation.
- Desktop is the current primary delivery form, but it now only maintains local mode — switching to a server connection from within the desktop client is no longer supported.

## Further Reading

- [Getting Started](/docs/en/start/desktop/)
- [Orchestration](/docs/en/surfaces/orchestration/)
- [Operations Overview](/docs/en/ops/)
- [Help](/docs/en/help/)
