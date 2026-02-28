# wunder
wunder is an agent orchestration system for organizations and individual users. It now has three runtime forms: server (cloud/service), cli (local terminal), and desktop (local GUI). Each form can run and ship independently. The server is the core: it supports multi-tenancy, user/org management, agent app building and publishing, unified gateway access and routing, and includes built-in tooling, knowledge bases, and long-term memory. The cli and desktop experiences are built on top of the same core.

<img width="1000" height="563" alt="wunder" src="https://github.com/user-attachments/assets/4e589030-f1fc-4e0c-91a7-3419eb39a046" />

## Core Idea
For developers, everything is an interface; for LLMs, everything is a tool.

- Built-in tools (dynamic): hands and feet
- MCP tools (dynamic): blades and swords
- Skills (static): workflow playbooks
- Knowledge tools (static): encyclopedias
- Custom tools (dynamic): personal gear
- Shared tools (dynamic): the gear marketplace

wunder can expose itself as a self-hosted MCP tool (`/wunder/mcp`) for cross-system usage.

<img width="700" height="380" alt="ayanami" src="https://github.com/user-attachments/assets/8ef1f7f9-f563-4253-8663-238c831d1aa3" />

## Runtime Forms & Capabilities
| Form | Best for | Core capabilities | Default persistence |
| --- | --- | --- | --- |
| `wunder-server` | Team collaboration, multi-tenant deployment, unified gateway access | Unified `/wunder` API, user/org governance, app publishing, channel access, monitoring & evaluation | `workspaces/<user_id>` + PostgreSQL (default) |
| `wunder-cli` | Local development, scripting, lightweight chat | Interactive TUI, `ask/chat/resume`, `exec/tool/mcp/skills/config/doctor`, JSONL event output, `tool_call/function_call` switching | `WUNDER_TEMP/` under launch dir (SQLite + config + sessions) |
| `wunder-desktop` | Local end users, visual operations | Tauri desktop window (optional Electron/AppImage) + local bridge service, reused user UI, MCP/Skills/tool management, WebSocket-first with SSE fallback | `WUNDER_TEMPD/` beside app + default workspace `WUNDER_WORK/` (Electron uses userData path) |

## Capability Matrix
### User Side (`frontend`)
- App plaza `/home`: create agent apps and browse shared apps.
- Chat as the default entry, with streaming intermediate events and final output.
- Workspace for files and artifacts, with resource preview support.
- Session history for replay and continuation.
- Light and dark themes are both supported.

### Admin Side (`web`)
- User/org/permission management and quota governance.
- Agent app lifecycle management (create, publish, share, retire).
- Model, tool, Skills, and MCP configuration/enablement.
- Unified gateway entry and policy routing (auth, rate limiting, audit).
- Debug & observability: session monitor, throughput tests, and performance sampling.

### Orchestration Core (Rust)
- Automatic context compaction plus optional long-term memory for stable long sessions.
- Multi-user isolation: `user_id` is both the session key and workspace key; virtual users are supported.
- Tool stack: built-in + MCP + Skills + knowledge + custom/shared tools.
- Session accounting split into user turns and model turns for better observability.
- Communication strategy is WebSocket first, with SSE as fallback.

## Entrypoints & Usage
### Role-based access (recommended)
- **Administrators**: use the admin frontend (`web`) for model/tool governance, permissions, monitoring, and operations.
- **Users**: choose the user web frontend (`frontend`), `wunder-desktop`, or `wunder-cli` based on the usage scenario.
- **Channel access**: users can also connect through channels (such as Feishu/WhatsApp/QQ), which route into the same orchestration pipeline.

### Server entrypoints (multi-tenant / platform deployment)
- Admin debug UI: `http://127.0.0.1:18000`
- User frontend (development, default): `http://127.0.0.1:18001`
- User frontend (production static, with Nginx): `http://127.0.0.1:18002`
- API entry: `/wunder` (streaming + non-streaming)
- MCP entry: `/wunder/mcp`

### Local forms at a glance (CLI / Desktop)
- `wunder-cli`: local terminal form; state is persisted in `WUNDER_TEMP/` under the launch directory.
- `wunder-desktop`: local GUI form; state is persisted in `WUNDER_TEMPD/` beside the app, with default workspace `WUNDER_WORK/` (Electron uses userData via `--temp-root/--workspace`).
- Startup examples for local forms are grouped in “Quick Start” to keep them clearly separated from server deployment.

### Session Control Commands
#### User frontend + channel inbound
- `/new` or `/reset`: create a new thread and switch to it.
- `/stop` or `/cancel`: request cancellation of the current run.
- `/help` or `/?`: return command help text.
- `/compact` (frontend only): trigger context compaction for current session.

Notes:
- Commands are case-insensitive; only the first token is parsed (for example, `/new hello` is treated as `/new`).
- Channel command parsing is in `src/channels/service.rs`; frontend command handling is in `frontend/src/views/ChatView.vue`.

#### CLI interactive commands (summary)
- `/help`, `/status`, `/model`, `/tool-call-mode` (`/mode`)
- `/session`, `/system`, `/config`
- `/new`, `/exit` (plus TUI-only commands such as `/mouse`)

## Quick Start
### Path A: `wunder-server` (multi-tenant / platform deployment)
#### 1) Configuration (optional)
- Works out of the box: no `.env` or `config/wunder.yaml` required; if `config/wunder.yaml` is missing, it automatically falls back to `config/wunder-example.yaml`.
- Only copy templates when you need overrides:
  - `config/wunder-example.yaml` -> `config/wunder.yaml`
  - `.env.example` -> `.env` (for `WUNDER_API_KEY`, `WUNDER_POSTGRES_DSN`, `WUNDER_SANDBOX_ENDPOINT`, etc.)
- Frontend API base: if needed, set `VITE_API_BASE` or `VITE_API_BASE_URL` in repo root `.env`, then restart frontend.

#### 2) Start the server
x86
```bash
docker compose -f docker-compose-x86.yml up
```
arm
```bash
docker compose -f docker-compose-arm.yml up
```
The first startup pulls base images and builds dependencies, so it may take some time.

#### 3) Open server entrypoints
- Admin debug UI: `http://127.0.0.1:18000`
- User frontend (development): `http://127.0.0.1:18001`
- User frontend (production static): `http://127.0.0.1:18002`

### Path B: `wunder-cli` (local terminal)
```bash
# Enter interactive mode (TUI by default on TTY)
cargo run --bin wunder-cli

# One-shot question
cargo run --bin wunder-cli -- ask "Summarize the current project structure"

# Resume the latest session
cargo run --bin wunder-cli -- resume --last

# List available tools
cargo run --bin wunder-cli -- tool list
```

### Path C: `wunder-desktop` (local GUI)
```bash
# Start desktop window (local bridge defaults to 127.0.0.1:18123; falls back to a random port if occupied)
cargo run --features desktop --bin wunder-desktop

# Start bridge with a random free port
cargo run --features desktop --bin wunder-desktop -- --port 0

# Bridge-only mode (no desktop window)
cargo run --features desktop --bin wunder-desktop -- --bridge-only --open

# Bridge-only binary (no Tauri, for Electron shell)
cargo run --bin wunder-desktop-bridge -- --open
```

## Persistence & Directory Conventions
- Server workspace: `workspaces/<user_id>` (use `/workspaces/<user_id>/` in prompts).
- Docker Compose uses two named volumes by default:
  - `wunder_workspaces`: mounted to `/workspaces` (user workspaces)
  - `wunder_logs`: mounted to PostgreSQL/Weaviate data dirs (`/var/lib/postgresql/data`, `/var/lib/weaviate`)
- `temp_dir` defaults to local `./temp_dir` (container: `/app/temp_dir`; override via `WUNDER_TEMP_DIR_ROOT`)
- Other runtime configs remain on the repo bind mount:
  - local `./data/config`, `./data/prompt_templates`, `./data/user_tools`, etc.
- Build/dependency caches (`target/`, `.cargo/`, `frontend/node_modules/`) stay on the repo bind mount for easier local cleanup/management.
- Note: `docker compose down -v` deletes `wunder_workspaces` and `wunder_logs` volumes; it won't delete the local `data/` directory in the repo.
- CLI persistence: `WUNDER_TEMP/` (SQLite, config overrides, sessions, user tool data).
- Desktop persistence: `WUNDER_TEMPD/`; default workspace `WUNDER_WORK/` (Electron uses userData path).
- Chat history, tool logs, and monitor events are persisted to DB (PostgreSQL by default for server, SQLite by default for local forms).
- Admin override path: `data/config/wunder.override.yaml` (runtime override file, safe to regenerate).

## Skills & MCP
- Skills are loaded from `skills/` by default, and can be enabled in `config/wunder.yaml` or via admin UI.
- MCP servers are configured in `config/wunder.yaml` or `data/config/wunder.override.yaml`, and managed in admin UI.
- server / cli / desktop share the same tool protocol and orchestration core for easier cross-form migration.

## Project Structure
```text
src/                 # Rust core services (API/orchestration/tools/storage)
  api/               # /wunder, /a2a, admin endpoints
  channels/          # external channel integrations
  gateway/           # gateway control-plane
  orchestrator/      # orchestration engine
  services/          # tools/LLM/MCP/workspace services
  storage/           # PostgreSQL/SQLite persistence
  core/              # config/auth/i18n/state
wunder-cli/          # CLI runtime form (TUI + commands)
wunder-desktop/      # Desktop runtime form (Tauri + local bridge)
wunder-desktop-electron/ # Electron shell (optional, AppImage friendly)
frontend/            # user frontend (Vue3)
web/                 # admin frontend (debug/governance)
config/              # base configuration
prompts/             # system/tool/memory prompts
skills/              # built-in skills
knowledge/           # knowledge base
scripts/             # dev & maintenance scripts
docs/                # design/API/plan documents
```

## Related Docs
- System overview: `docs/系统介绍.md`
- Design plan: `docs/设计方案.md`
- API docs: `docs/API文档.md`
- wunder-cli design: `docs/方案/wunder-cli实现方案.md`
- wunder-desktop design: `docs/方案/wunder-desktop实现方案.md`
- Test plan: `docs/方案/测试方案.md`

## wunder Devoured Core
| Devoured | Project Name | GitHub URL |
| :--- | :--- | :--- |
| Agent Foundation | EVA | https://github.com/ylsdamxssjxxdd/eva |
| Rust Foundation | OpenAI Codex | https://github.com/openai/codex |
| Frontend Foundation | HuLa | https://github.com/HuLaSpark/HuLa |
| MCP/SKILLS | Claude Code | https://github.com/anthropics/claude-code |
| Gateway/Channel/Cron Jobs | OpenClaw | https://github.com/openclaw/openclaw |
| Agent LSP | OpenCode | https://github.com/anomalyco/opencode |
