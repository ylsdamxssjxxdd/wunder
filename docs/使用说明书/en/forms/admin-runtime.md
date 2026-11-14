---
title: Admin Runtime
nav_title: Runtime
summary: The Admin has no process of its own: static pages served by the Server, API and MCP entry points on the same port, plus the browser and account it requires.
updated_at: 2026-11-14
---

# Admin Runtime

The Admin is not a program you start separately: it is a set of static pages (plain HTML plus JS modules) shipped by the Server. Its runtime is the Server itself.

## What it is made of

| Part | Where | Notes |
| --- | --- | --- |
| Pages | the Server's static directory | Entry `http://<host>:18000/`, no build step — an edit is live on reload |
| API | same port, `/wunder/*` | The governance and debugging endpoints the pages call |
| MCP endpoint | same port | For external tools to connect |
| External workflow entry | same port | For external flows to trigger |

No separate process, no separate port, no database of its own: reads and writes go through the Server's existing storage.

## Prerequisites

- A working Server deployment, see [Cloud Runtime](/docs/en/forms/cloud-runtime/).
- A recent Chrome, Edge, Firefox, or Safari. The pages are dense with tables, logs, and monitoring panels: they stay readable on a narrow screen but are slow to work in.
- An administrator account, or a unit owner (who manages only their own unit and its users). Change the default password right after the first login.

## How it behaves

- Data-dense by design: tables, logs, and monitoring panels are paginated, filtered, or capped, so no view dumps the whole dataset at once.
- High-risk actions expect a clear confirmation boundary, and a front-end confirmation never replaces back-end authorization.
- There is no goal editor and no end-user chat: goals are handled in Cloud, Desktop, and the CLI.

## When it does not load

If the page will not open, check whether the Server is listening on 18000. If it opens but calls fail, read the Server log for authorization and routing entries — the pages hold no cache and do not hide API errors.
