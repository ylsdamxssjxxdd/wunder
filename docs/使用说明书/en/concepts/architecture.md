---
title: Architecture
summary: All four forms share the same core and engine; real execution happens inside sessions, and the interface reads a bounded display view.
updated_at: 2026-10-13
---

All four forms share the same core and engine; real execution happens inside sessions, and the interface reads a bounded display view.

## Four forms, each with its own delivery surface

| Form | How it is delivered | Interface |
| --- | --- | --- |
| Pupa | A desktop app; the engine ships inside the local process and calls in-process by default without local HTTP | Native UI, compiled into the app |
| Beehive | A web app; its build artifacts are served read-only by a separate web service, not hosted by the Hull itself | Single-page app |
| Helm | A command-line program; when terminal conditions are not met it degrades to one-shot execution instead of the full-screen interactive interface | Terminal UI |
| Hull | A server process, mounting Bridge and a few static directories | Bridge (no build step) |

There is only one core and one engine; the four forms differ only at the access layer.

## What a message passes through

From the moment you send a message to the moment the final reply is stored, the order is fixed:

| Step | What it does |
| --- | --- |
| Access | The main chat channel takes the request and can submit, resume, observe, cancel, and approve it |
| Admission | The session runtime decides whether to run it immediately or queue it — the single decision point for "dispatch or queue" |
| Record the user round | A user round is allocated idempotently; this is the only write entry for the session record |
| Prepare the request | Context, visible tools, and the system prompt are assembled, and a decision is made on whether to freeze |
| Main loop | Every model call increments the model round by one |
| Run tools | Policy and approval, hooks, timeouts, and budget come first, then the call is dispatched to the actual tool |
| Final reply | Multiple refreshes within the same model round update the same visible output item and never append duplicate bubbles |
| Real-time push | Emitting an event writes to the persistence layer synchronously, and subscribers are notified only after the write completes |

Three places that are easy to mislocate:

- The status registry the interface queries for a session's current state is a different thing from the session runtime that actually runs execution.
- In-process wake signals only exist to accelerate; across multiple instances, correctness comes from polling convergence, not from them.
- The four phases of a tool — pre, execute, post, result — are only a descriptive model; hooks are off by default and are not part of the execution main chain.

## Data and storage

- Storage is split by purpose: thread logs, sessions, workspaces, memory fragments, and so on each form their own group, and the engine depends only on this set of interfaces without caring which database sits underneath.
- For the choice of database: with no configuration it is SQLite, and configured as PostgreSQL it runs on PostgreSQL.
- There is no version table and no migration-script directory. Tables are created idempotently on startup and new columns are detected at startup, so there is no such thing as migrating by version number.
- Every kind of record — sessions, round tails, workspaces, media attachments, memory fragments, knowledge vectors, users and quotas, channel bindings, interop shadows — is defined together.

## What the interface sees is a bounded view, not an execution snapshot

What you see is a bounded display view, not a full history you can page through to the end, and not a complete snapshot of execution.

| View | Per page | Cap | Response budget |
| --- | --- | --- | --- |
| Chat page (one row per user round) | 12 | 20 | 128 KB |
| Thread trace page (per entry) | 80 | 100 | 192 KB |
| Body chunks | 8 KB per chunk | — | fetched on demand |

The mechanisms that go with it: a write also produces a projection table and a truncated preview; when a projection is not ready it falls back to a bounded window over the source table; background rebuilds stay at no more than 1% duty cycle; and each change push fetches 200 items. Do not mistake the machinery that measures historical usage and budgets for context compression for a pagination module — it does not handle pagination.

## External interfaces

| Surface | Fact |
| --- | --- |
| HTTP | Routes share the `/wunder/*` prefix, and error responses have exactly one source format |
| WebSocket | Six channels: chat, the core request surface, gateway, interop tunnel, interop remote observer, and world / LAN |
| Gateway | Not an HTTP gateway, but an access registry plus targeted RPC plus online-presence broadcast; its only transport is `/wunder/gateway/ws` |
| Channels | Adapter registration; a bounded inbound queue, and a persistent outbound outbox with backoff retry |
| Interop | Local nodes are outbound-only; one tunnel carries three planes; capabilities are tiered L0–L3, and the default device capability set excludes L3 |
| MCP | The Hull has a built-in service endpoint at `/wunder/mcp` (streamable-http), and external MCP clients are registered in configuration |

## Runtime defaults

| Item | Default |
| --- | --- |
| Hull listen | `0.0.0.0:8000`, overridable by environment variable or configuration |
| Active session cap | 300 (unlimited for administrators) |
| Session lock | 120 s timeout, 5 s heartbeat |
| Model / tool timeout | 120 s each |
| Model round cap | 1000 for non-administrators |

## Common misconceptions

- **"The Beehive page is hosted by the Hull."** No. The Hull mounts only Bridge and a few static directories; Beehive's build artifacts are served by a separate web service.
- **"The front end needs several fetches to get the full history."** The front end was never meant to get the full history; it consumes a paginated view and fetches body chunks on demand.
- **"Pupa calls its own backend over local HTTP."** By default it is a native in-process call; the standalone compatibility bridge is kept only for existing clients.
- **"Swarm orchestration is still part of the architecture."** The engine and scheduling surface no longer have this module; only an empty directory remains, and session write permission is always writable.

## Further reading

- [Agent Loop](/docs/en/concepts/core-agent-loop/) — the main loop and braking
- [Sessions and Rounds](/docs/en/concepts/sessions-and-rounds/) — the numbering conventions
- [Streaming Execution](/docs/en/concepts/streaming/) — push, refill, and backpressure
- [Workspaces and Working Directories](/docs/en/concepts/workspaces/) — paths and sandbox boundaries
- [Configuration Reference](/docs/en/reference/config/) — every configuration option
