# wunder
wunder is a multi-tenant agent scheduling system for organizations and users. It supports user/org management, agent app creation and publishing, a gateway for unified access and scheduling, and built-in tooling, knowledge bases, and optional long-term memory. The Rust (Axum) service exposes a unified `/wunder` entry, supports streaming and non-streaming responses, and ships with user and admin frontends, a debug console, and management APIs.
<img width="1000" height="563" alt="wunder" src="https://github.com/user-attachments/assets/4e589030-f1fc-4e0c-91a7-3419eb39a046" />

## Core Idea
For developers, everything is an interface; for LLMs, everything is a tool.
- Built-in tools (dynamic): hands and feet
- MCP tools (dynamic): swords and blades
- Skills (static): walkthrough playbooks
- Knowledge tools (static): encyclopedias
- Custom tools (dynamic): personal gear
- Shared tools (dynamic): the gear market

wunder can expose itself as a self-hosted MCP tool (`/wunder/mcp`) for cross-system usage.

## 1. Capability Matrix
### User Side (App Users)
- App plaza `/home`: create agent apps, browse shared apps.
- Chat as the default entry with streaming process and final answer.
- Workspace for files and artifacts with previews.
- History and resume.
- Light/dark themes in the user frontend.

### Admin Side (Org & Ops)
- User/org/permission management plus quota governance.
- Agent app lifecycle management (create, publish, share, retire).
- Model, tool, Skills, and MCP catalog management and enablement.
- Gateway for unified access and policy routing (auth, rate limits, audit).
- Monitoring, throughput tests, and performance sampling.

### Scheduling & Platform
- Automatic context compaction + optional long-term memory for long sessions.
- Multi-user isolation: `user_id` is the session/workspace key and can be virtual.
- Tooling: built-in + MCP + Skills + knowledge + custom/shared tools.
- UI and system prompts support language switching.

## 2. Entrypoints & Usage
- Admin debug UI: `http://127.0.0.1:18000`
- User frontend: `http://127.0.0.1:18001`
- Unified API entry: `/wunder` (streaming + non-streaming)

Usage flow:
1. Start the services and open the user frontend: `http://127.0.0.1:18001`
2. Enter `/home` to create or select an agent app (or go straight to chat).
3. Use chat to interact; prepare required files in the workspace.

## 3. Quick Start
### 3.1 Update configuration
Copy the example config: `config/wunder-example.yaml` -> `config/wunder.yaml`
Copy env example: `.env.example` -> `.env` and set `WUNDER_API_KEY`, `WUNDER_POSTGRES_DSN`, `WUNDER_SANDBOX_ENDPOINT`, etc.
Frontend API base: set `VITE_API_BASE` or `VITE_API_BASE_URL` in the repo root `.env` and restart the frontend.

### 3.2 Start the service
x86
```bash
docker compose -f docker-compose-x86.yml up
```
arm
```bash
docker compose -f docker-compose-arm.yml up
```
The first start pulls base images and builds dependencies, so it may take a while.

### 3.3 Open the admin debug UI
Open in browser:
```
http://127.0.0.1:18000
```

### 3.4 Open the user frontend
Open in browser:
```
http://127.0.0.1:18001
```

## 4. Request Examples
### 4.1 Non-stream request
```
curl -X POST http://127.0.0.1:18000/wunder ^
  -H "Content-Type: application/json" ^
  -H "X-API-Key: <your-api-key>" ^
  -d "{"user_id":"u001","question":"Hello","stream":false}"
```

### 4.2 Streaming SSE request
```
curl -N -X POST http://127.0.0.1:18000/wunder ^
  -H "Content-Type: application/json" ^
  -H "X-API-Key: <your-api-key>" ^
  -H "Accept: text/event-stream" ^
  -d "{"user_id":"u001","question":"Hello","stream":true,"debug_payload":true}"
```
`debug_payload` works only on `/wunder` (`/wunder/chat` omits full request bodies).

Common SSE event types include:
`progress`, `llm_request`, `llm_output_delta`, `llm_output`, `tool_call`, `tool_output_delta`, `tool_result`, `token_usage`, `context_usage`, `plan_update`, `question_panel`, `a2ui`, `final`, `error`

### 4.3 Enable tools on demand
```
curl -X GET "http://127.0.0.1:18000/wunder/tools?user_id=u001" ^
  -H "X-API-Key: <your-api-key>"
```

## 5. API Entry Overview
Core endpoints:
- `POST /wunder`
- `POST /wunder/system_prompt`
- `GET /wunder/tools`
- `GET /wunder/i18n`

User-facing:
- `/wunder/auth`
- `/wunder/chat/*`
- `/wunder/workspace/*`
- `/wunder/user_tools/*`

Admin & ops:
- `/wunder/admin/*`
- `/wunder/admin/throughput/*`
- `/wunder/admin/evaluation/*`
- `/wunder/admin/performance/sample`
- `/wunder/admin/memory/*`

Other entries:
- `/a2a` + `/.well-known/agent-card.json`
- `/wunder/mcp`
- `/wunder/doc2md/convert`
- `/wunder/attachments/convert`
- `/wunder/temp_dir/*`

See `docs/API-Documentation.md` for details.

## 6. Workspace & Persistence
- Workspace path: `workspaces/<user_id>` (prompt uses `/workspaces/<user_id>/`).
- Chat history/tool logs/monitor/locks/overflow events are stored in the database (PostgreSQL by default; SQLite optional).
- Legacy `data/historys/` is kept for migration only.
- Admin overrides are stored in `data/config/wunder.override.yaml`.
- Throughput reports are stored in `data/throughput`.

Concurrent requests for the same `user_id` are rejected (HTTP 429).

## 7. Skills & MCP
### 7.1 Skills
- Skills are loaded from `skills/` and `EVA_SKILLS/` by default.
- `SKILL.md` must include YAML frontmatter with `name/description/input_schema` (Chinese `??/??/????` also supported).
- Entrypoints: `run.py` / `skill.py` / `main.py`, using `run(payload)`.
- Enable via `config/wunder.yaml` or `/wunder/admin/skills`.

### 7.2 MCP
- Configure `mcp.servers` in `config/wunder.yaml` and `data/config/wunder.override.yaml`.
- Manage via `/wunder/admin/mcp` and `/wunder/admin/mcp/tools`.

## 8. Project Structure
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
data/throughput/    # throughput reports
docs/               # design/API/test docs
```

## 9. Related Docs
- System overview: `docs/System-Overview.md`
- Design plan: `docs/Design-Plan.md`
- API documentation: `docs/API-Documentation.md`
- Request samples: `docs/Request-Examples.md`
- Test plan: `docs/Test-Plan.md`
