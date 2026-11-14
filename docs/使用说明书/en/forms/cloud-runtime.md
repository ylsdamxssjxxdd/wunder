---
title: Cloud Runtime
nav_title: Runtime
summary: The services and containers Cloud depends on, the port map, database and memory floor, and what the client browser has to be.
updated_at: 2026-11-14
---

# Cloud Runtime

Cloud is the workbench in a browser and is not a process of its own: the pages are served read-only by a separate web container, while data, models and tool execution live in the Server. Its runtime is therefore "one Server deployment".

## What it is made of

| Service | Default port | Role |
| --- | --- | --- |
| Server | 18000 | Agent scheduling, tool execution, API (`/wunder/*`) |
| Cloud pages | 18001 | Static pages served by the dedicated web container |
| Sandbox | 9001 | Isolated environment for commands and code |
| PostgreSQL | 5432 | Business data; SQLite is an option for single-host setups |
| OnlyOffice | 18080 | In-browser document editing |
| drawio | 18081 | In-browser diagrams |
| extra-mcp | 18004 | Additional MCP service entry |

Ports can be overridden: server `WUNDER_HOST` / `WUNDER_PORT`, cloud pages `NGINX_PORT`.

## Resource floor

- At least 4 GB of free memory: the server, the database and the document services run together, and the headroom decides whether concurrency queues.
- Disk follows session and workspace size; attachments, screenshots and document caches all sit server-side.
- Timezone defaults to `Asia/Shanghai`; change it to match the deployment.

## Database

PostgreSQL by default, started with the compose file. Small single-host deployments can switch to SQLite and drop one container, at the cost of different concurrent-write and backup behaviour.

## Client requirements

- Browsers: current Chrome, Edge, Firefox or Safari; the pages are a plain Vue 3 + TypeScript SPA.
- No plugin or client program, and no local Python or browser engine — those live in the server and the sandbox.
- Device interlink, voice and screenshot capture depend on browser APIs; see [Cloud Usage](/docs/en/forms/cloud-usage/) for the boundaries.

## How it relates to the other forms

- The Admin shares the server port; see [Admin Runtime](/docs/en/forms/admin-runtime/).
- Commands and file access in the sandbox reuse the engine, so tool semantics match Desktop; the difference is that execution happens inside a server container, not on the user's machine.
