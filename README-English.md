# wunder
wunder is a multi-user agent orchestration platform that connects LLM APIs, MCP tools, and Skills workflows, with a built-in knowledge base. The Rust (Axum) server exposes a unified `/wunder` entry, supports SSE streaming and non-streaming responses, and ships with a debug console plus admin APIs.
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

## 1. Feature Overview
- Unified entry `/wunder`: supports SSE streaming and non-streaming responses.
- A2A standard API `/a2a` + `/.well-known/agent-card.json` for capability discovery.
- Tooling: built-in + MCP + Skills + knowledge + custom/shared tools, enabled via `tool_names`.
- Automatic context compaction + optional long-term memory for long sessions.
- Multi-user isolation: `user_id` is the session/workspace key and can be virtual.
- Quota governance: registered users consume per model call; virtual `user_id`s are not limited.
- Debugging & monitoring: `/` console + `/wunder/admin/monitor` for session stats.
- Throughput tests, performance sampling, and capability evaluation built-in.
- Language switching for UI + system prompts.

## 2. Quick Start
### 2.1 Update configuration
Copy the example config: `config/wunder-example.yaml` -> `config/wunder.yaml`
Copy env example: `.env.example` -> `.env` and set `WUNDER_API_KEY`, `WUNDER_POSTGRES_DSN`, `WUNDER_SANDBOX_ENDPOINT`, etc.
Frontend API base: set `VITE_API_BASE` or `VITE_API_BASE_URL` in the repo root `.env` and restart the frontend.

### 2.2 Start the service
x86
```bash
docker compose -f docker-compose-x86.yml up
```
arm
```bash
docker compose -f docker-compose-arm.yml up
```
The first start pulls base images and builds dependencies, so it may take a while.

### 2.3 Open the admin debug UI
Open in browser:
```
http://127.0.0.1:18000
```

### 2.4 Open the user frontend
Open in browser:
```
http://127.0.0.1:18001
```

## 3. Request Examples
### 3.1 Non-stream request
```
curl -X POST http://127.0.0.1:18000/wunder ^
  -H "Content-Type: application/json" ^
  -H "X-API-Key: <your-api-key>" ^
  -d "{"user_id":"u001","question":"Hello","stream":false}"
```

### 3.2 Streaming SSE request
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

### 3.3 Enable tools on demand
```
curl -X GET "http://127.0.0.1:18000/wunder/tools?user_id=u001" ^
  -H "X-API-Key: <your-api-key>"
```

## 4. API Entry Overview
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

## 5. Workspace & Persistence
- Workspace path: `workspaces/<user_id>` (prompt uses `/workspaces/<user_id>/`).
- Chat history/tool logs/monitor/locks/overflow events are stored in the database (PostgreSQL by default; SQLite optional).
- Legacy `data/historys/` is kept for migration only.
- Admin overrides are stored in `data/config/wunder.override.yaml`.
- Throughput reports are stored in `data/throughput`.

Concurrent requests for the same `user_id` are rejected (HTTP 429).

## 6. Skills & MCP
### 6.1 Skills
- Skills are loaded from `skills/` and `EVA_SKILLS/` by default.
- `SKILL.md` must include YAML frontmatter with `name/description/input_schema` (Chinese `??/??/????` also supported).
- Entrypoints: `run.py` / `skill.py` / `main.py`, using `run(payload)`.
- Enable via `config/wunder.yaml` or `/wunder/admin/skills`.

### 6.2 MCP
- Configure `mcp.servers` in `config/wunder.yaml` and `data/config/wunder.override.yaml`.
- Manage via `/wunder/admin/mcp` and `/wunder/admin/mcp/tools`.

## 7. Project Structure
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

## 8. Related Docs
- System overview: `docs/System-Overview.md`
- Design plan: `docs/Design-Plan.md`
- API documentation: `docs/API-Documentation.md`
- Request samples: `docs/Request-Examples.md`
- Test plan: `docs/Test-Plan.md`
