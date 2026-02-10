# wunder
wunder is an agent scheduling system for organizations and users. It supports multi-tenancy, user and organization management, agent app creation and publishing, a gateway for unified access and scheduling, and built-in tooling, knowledge bases, and long-term memory capabilities. The Rust (Axum) service exposes a unified `/wunder` entry, supports streaming and non-streaming responses, and ships with user and admin frontends, a debug console, and management APIs.
<img width="1000" height="563" alt="wunder" src="https://github.com/user-attachments/assets/4e589030-f1fc-4e0c-91a7-3419eb39a046" />

## Core Idea
For developers, everything is an interface; for LLMs, everything is a tool.
- Built-in tools (dynamic): hands and feet
- MCP tools (dynamic): knives and swords
- Skills (static): workflow handbooks
- Knowledge tools (static): encyclopedias
- Custom tools (dynamic): personal gear
- Shared tools (dynamic): the gear marketplace

wunder can expose itself as a self-hosted MCP tool (`/wunder/mcp`) for cross-system usage.

<img width="700" height="380" alt="ayanami" src="https://github.com/user-attachments/assets/8ef1f7f9-f563-4253-8663-238c831d1aa3" />

## Capability Matrix
### User Side (App Users)
- App plaza `/home`: create agent apps and browse shared apps.
- Chat as the default entry with streaming process and final answer display.
- Workspace for files and artifacts with previews.
- History and resume.
- Light/dark themes in the user frontend.

### Admin Side (Org & Ops)
- User/org/permission management and quota governance.
- Agent app lifecycle management (create, publish, share, retire).
- Model, tool, Skills, and MCP management and enablement.
- Gateway for unified entry and policy routing (auth, rate limits, audit).
- Debugging & monitoring: session monitoring, throughput tests, and performance sampling.

### Scheduling & Platform
- Automatic context compaction + optional long-term memory for long sessions.
- Multi-user isolation: `user_id` is the session/workspace key and can be virtual.
- Tooling: built-in + MCP + Skills + knowledge + custom/shared tools.
- UI and system prompts support language switching.

## Entrypoints & Usage
- Admin debug UI: `http://127.0.0.1:18000`
- Debug frontend: `http://127.0.0.1:18001`
- User frontend (development, default): `http://127.0.0.1:18001`
- User frontend (production static, when Nginx is enabled): `http://127.0.0.1:18002`
- Unified API entry: `/wunder` (streaming + non-streaming)

### Usage flow
1. After startup, open the user frontend (development default): `http://127.0.0.1:18001` (use `http://127.0.0.1:18002` when Nginx static deployment is enabled)
2. Enter `/home` to create or select an agent app (or go straight to chat).
3. Use chat to interact; prepare required files in the workspace.

### Session Control Commands (User Frontend + Channel Inbound)
Session control commands are available in both the user chat input and channel inbound adapters:

- `/new` or `/reset`: create a new thread and switch to it.
- `/stop` or `/cancel`: request cancellation of the current running session.
- `/help` or `/?`: return command help text.

User frontend also supports:

- `/compact`: trigger context compaction for the current session.

Notes:
- Commands are case-insensitive and only the first token is parsed (for example, `/new hello` is treated as `/new`).
- Channel command parsing is in `src/channels/service.rs`; user-side command handling is in `frontend/src/views/ChatView.vue`.

## Quick Start
### 1) Update configuration
Copy the example config: `config/wunder-example.yaml` -> `config/wunder.yaml`
Copy env example: `.env.example` -> `.env` and set `WUNDER_API_KEY`, `WUNDER_POSTGRES_DSN`, `WUNDER_SANDBOX_ENDPOINT`, etc.
Frontend API base: set `VITE_API_BASE` or `VITE_API_BASE_URL` in the repo root `.env` and restart the frontend.

### 2) Start the service
x86
```bash
docker compose -f docker-compose-x86.yml up
```
arm
```bash
docker compose -f docker-compose-arm.yml up
```
The first start pulls base images and builds dependencies, so it may take a while.

### 3) Open entrypoints
Admin debug UI: `http://127.0.0.1:18000`
User frontend (development, default): `http://127.0.0.1:18001`
User frontend (production static, when Nginx is enabled): `http://127.0.0.1:18002`

## Workspace & Persistence
- Workspace path: `workspaces/<user_id>` (prompt uses `/workspaces/<user_id>/`).
- Chat history/tool logs/monitor/locks/overflow events are stored in the database (PostgreSQL by default; SQLite optional).
- Admin overrides are stored in `data/config/wunder.override.yaml`.

## Skills & MCP
- Skills load from `skills/` and `EVA_SKILLS/` by default, enabled via `config/wunder.yaml` or the admin UI.
- MCP is configured in `config/wunder.yaml` or `data/config/wunder.override.yaml`, with the tool catalog maintained by the admin UI.

## Project Structure
```
src/                 # Rust server modules
  api/               # /wunder, /a2a, admin APIs
  core/              # config/auth/i18n
  services/          # tools/LLM/MCP/workspace
  ops/               # monitor/evaluation/throughput
  sandbox/           # sandbox client/server
  orchestrator/      # orchestration engine
  storage/           # PostgreSQL/SQLite persistence
config/              # base configuration
prompts/             # system/tool/memory prompts
workspaces/          # user workspaces
skills/              # built-in skills
EVA_SKILLS/          # skills directory
knowledge/           # knowledge bases
temp_dir/            # temp files
web/                 # admin debug UI
frontend/            # user frontend (Vue3)
data/config/         # admin overrides
data/throughput/     # throughput reports
docs/                # design/API/test docs
```

## Related Docs
- System overview: `docs/系统介绍.md`
- Design plan: `docs/设计方案.md`
- API documentation: `docs/API文档.md`
- Test plan: `docs/方案/Test-Plan.md`

## wunder Devoured Core
- EVA: <https://github.com/ylsdamxssjxxdd/eva>
- OpenAI Codex: <https://github.com/openai/codex>
- Claude Code: <https://github.com/anthropics/claude-code>
- OpenClaw: <https://github.com/openclaw/openclaw>
- OpenCode: <https://github.com/anomalyco/opencode>

