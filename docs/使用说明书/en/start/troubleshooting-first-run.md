---
title: First-Run Troubleshooting
summary: The failure modes and verification points for the first launch or deployment of each form: Desktop startup, Server deployment, Cloud access, CLI execution.
read_when:
  - A first install, launch, or deployment failed
  - You need verification points to confirm a form is healthy
source_docs:
  - crates/wunder-desktop/native.rs
  - crates/wunder-server/src/main.rs
  - crates/wunder-cli/main.rs
  - crates/wunder-cli/runtime.rs
  - docker-compose-x86.yml
updated_at: 2026-10-16
---

# First-Run Troubleshooting

Sections are organized by form. Each section's verification points are a health checklist for that form; the failure table gives what to check first and where to go next.

## Desktop

### Verification points

- After `wunder-desktop` starts you land in the workbench, with **no** local listening port at all
- `~/.wunder/workspace` and `<temp>/config/desktop.settings.json` have been created
- Settings → Model: "Test connection" returns success
- The first message shows tool batches and a final reply

### Failure symptoms and where to check

| Symptom | Check first | Docs |
| --- | --- | --- |
| The startup screen never finishes | Permissions on `~/.wunder`; work-tree scan duration (up to 4096 items) | [Desktop intro](/docs/en/start/desktop/) |
| The window appears but the workspace is empty | Whether the working-directory area shows "offline" | [Desktop interface](/docs/en/surfaces/desktop-ui/) |
| Model request 401/403 | Settings → Model → API key and endpoint URL; rerun "Test connection" | [Configuration reference](/docs/en/capabilities/config/) |
| Sending a message during a run is rejected as "user busy" | The session queue is on by default; turning it off breaks the queue bar | [Quick Start](/docs/en/start/quickstart/) |
| Expecting TTS playback but hearing nothing | **Desktop TTS is not implemented** — only the model can be configured; use the Cloud for TTS | [Desktop interface](/docs/en/surfaces/desktop-ui/) |
| Can't find a "device interlink" settings category | The category isn't wired to the interface; the available surfaces are only the sidebar's cloud workspaces plus the remote command approval dialog | [Surfaces overview](/docs/en/surfaces/index/) |
| State is a mess | Settings → General → one-click workspace state reset (keeps skills, knowledge bases, long-term memory, configuration, and workspace files) | [Desktop intro](/docs/en/start/desktop/) |

## Server

### Verification points

- `docker compose ps` shows everything running: `wunder-server`, `wunder-nginx`, `postgres`, `sandbox`, `onlyoffice`, `drawio`, `extra-mcp` all present
- Health check: `GET http://<host>:18000/wunder/i18n` returns 200
- Admin: opening `http://<host>:18000/` in a browser shows the "4 groups / 19 items" top-level navigation
- Cloud: opening `http://<host>:18001/` in a browser reaches the login page

### Failure symptoms and where to check

| Symptom | Check first | Docs |
| --- | --- | --- |
| Port 18000 unreachable | `WUNDER_HOST` / `WUNDER_PORT` overrides; the server section of `config/wunder.yaml` | [Server deployment](/docs/en/start/server/) |
| The Admin loads but 18001 (Cloud) is blank | Whether the Cloud frontend was built (`frontend/dist`); whether the web-service container is up; `docker compose logs wunder-nginx` | [Server deployment](/docs/en/start/server/) |
| Opening 18000 shows the Cloud's look | The two concepts got mixed up — the Server only mounts the Admin static pages; the Cloud's `frontend/dist` is delivered by a standalone web-service container on 18001, not hosted by the Server | [Surfaces overview](/docs/en/surfaces/index/) |
| Loading the example config at startup silently fails and falls back to defaults | Known issue; point explicitly at `config/wunder.yaml`, or use `WUNDER_CONFIG_PATH` | [Configuration reference](/docs/en/capabilities/config/) |
| Client WebSocket keeps hitting `slow_client` | Slow-client backpressure (queue watermark 2); first check whether long messages are involved | [Troubleshooting](/docs/en/help/troubleshooting/) |
| PostgreSQL connection failure | `WUNDER_POSTGRES_DSN` / `WUNDER_POSTGRES_POOL_SIZE`; whether the `postgres` service is healthy | [Server deployment](/docs/en/start/server/) |
| Want to switch back to SQLite | `WUNDER_STORAGE_BACKEND=sqlite` + `WUNDER_STORAGE_DATA_ROOT=./runtime-data` | [Configuration reference](/docs/en/capabilities/config/) |
| Started in sandbox mode and it listens on 9001 | Normal: sandbox mode takes a different path | [Server deployment](/docs/en/start/server/) |

## Cloud (web)

### Verification points

- The browser opens `http://<host>:18001/` (the web-service container)
- After a successful login it navigates to `/app/chat` automatically
- The composer's "used/capacity" indicator updates normally
- "Work process" entries in the timeline can be expanded

### Failure symptoms and where to check

| Symptom | Check first | Docs |
| --- | --- | --- |
| Stuck in a `/login` loop after opening | Login tokens are opaque random strings, **not JWTs**; replaying one triggers whole-family revocation | Authentication and security |
| Long threads open slowly, rendering stutters | The high-volume thread rendering optimization plan has not landed yet; the performance bottleneck remains | [Cloud interface](/docs/en/surfaces/frontend/) |
| WebSocket drops and doesn't recover | After a disconnect, replay endpoints can backfill; the Cloud reuses a single `/chat/ws` channel | [Cloud interface](/docs/en/surfaces/frontend/) |
| Attachment upload fails | Temporary attachment limit 200 MB, persistent attachments 10 MB | [Cloud interface](/docs/en/surfaces/frontend/) |

## CLI

### Verification points

- In an interactive terminal, `wunder-cli` opens the ratatui TUI session
- Non-TTY (pipe / CI) degrades automatically to a one-shot run and exits cleanly
- `wunder-cli doctor` reports green
- Every line of `wunder-cli exec --json "..."` is valid JSON

### Failure symptoms and where to check

| Symptom | Check first | Docs |
| --- | --- | --- |
| The interactive session doesn't start; it exits as a one-shot run instead | Whether stdin/stdout/stderr are **all** TTYs | [CLI usage](/docs/en/start/cli/) |
| Permission error on `~/.wunder` | Override with `WUNDER_HOME`; the data directory defaults to `~/.wunder` | [CLI usage](/docs/en/start/cli/) |
| Can't find a `/cron` command | The CLI explicitly turns scheduled tasks off; there is no channel / knowledge / memory / persona management interface either | [CLI usage](/docs/en/start/cli/) |
| JSONL events missing fields | Event names are `thread` / `turn` / `item.*`; parse against that contract | [CLI usage](/docs/en/start/cli/) |
| Want to run a single tool in a script | Use `wunder-cli tool ...` directly; it doesn't go through the agent loop | [CLI usage](/docs/en/start/cli/) |
| A second message during a run is rejected as "user busy" | Expected: the CLI sets `config.agent_queue.enabled = false` (`crates/wunder-cli/runtime.rs`) — messages don't queue, they hit the session lock; use Desktop or Cloud to queue | [CLI usage](/docs/en/start/cli/) |

## General checks

- **Don't read "the frontend shells are no longer maintained" as "the Desktop backend stopped being maintained"**: the Electron / Tauri frontend shells stopped getting iterations; the engine and the Desktop backend continue to be developed.
- Lists, logs, and panels all have paging, filtering, or bounded limits. If a page renders a large amount of content at once and stutters, that is a known engineering gap — report it.

