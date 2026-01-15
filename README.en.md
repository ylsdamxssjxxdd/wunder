# wunder
wunder is a multi-user agent orchestration platform rebuilt from eva. It connects LLM APIs, MCP tools, and Skills workflows, and provides a basic lexical knowledge base. It exposes a single FastAPI entry at `/wunder`, streams intermediate progress and final answers, and ships a debug console plus basic admin endpoints.
<img width="1000" height="563" alt="wunder" src="https://github.com/user-attachments/assets/4e589030-f1fc-4e0c-91a7-3419eb39a046" />

## Core Idea
For developers, everything is an interface; for LLMs, everything is a tool.
- Built-in tools (dynamic): hands and feet
- MCP tools (dynamic): swords and blades
- Skills tools (static): walkthrough playbooks
- Knowledge tools (static): encyclopedias
- Custom tools (dynamic): personal gear
- Shared tools (dynamic): the gear market

wunder can expose itself as an MCP tool and become the ultimate tool.

## 1. Feature Overview
- Built-in LLM-driven automation pipeline + flexible prompt builder + automatic context compaction.
- Unified entry `/wunder`: streaming SSE and non-stream responses.
- Tooling: built-in tools + MCP tools + Skills, precisely enabled via `tool_names`.
- Multi-user isolation: a dedicated workspace per user_id with persistence.
- Debugging & monitoring: `/wunder/web` console and `/wunder/admin/monitor` resource/session metrics.

## 2. Quick Start
### 2.1 Build base image
x86
```bash
docker buildx build --platform linux/x86_64 -t wunder:20250105-x86 -f Dockerfile .
```
arm
```bash
docker buildx build --platform linux/arm64 -t wunder:20250105-arm64 -f Dockerfile .
```

### 2.2 Update configuration
Copy the example config to the real config: `config/wunder-example.yaml` -> `config/wunder.yaml`
Copy the env example: `.env.example` -> `.env`
Update `WUNDER_API_KEY` (and any other overrides) in `.env`.

### 2.3 Start the service
```bash
docker compose up
```
The first `docker compose up` downloads images and dependencies and may take a few minutes, so it is normal to see sparse logs initially.


### 2.4 Open the settings panel
Open in browser:
```
http://127.0.0.1:18000/wunder/web
```
Go to the Settings page, fill in the API base and key, and it will connect to the backend.

### 2.5 Open the model config panel
Add a model and save. You can use the probe button to get the max context length.

### 2.6 Open the debug panel
Go to Debug and send a test question.

## 3. Request Examples
### 3.1 Non-stream request
```
curl -X POST http://127.0.0.1:18000/wunder ^
  -H "Content-Type: application/json" ^
  -H "X-API-Key: <your-api-key>" ^
  -d "{\"user_id\":\"u001\",\"question\":\"Hello\",\"stream\":false}"
```

### 3.2 Streaming SSE request
```
curl -N -X POST http://127.0.0.1:18000/wunder ^
  -H "Content-Type: application/json" ^
  -H "X-API-Key: <your-api-key>" ^
  -d "{\"user_id\":\"u001\",\"question\":\"Hello\",\"stream\":true}"
```

SSE event types include:
`progress`, `llm_request`, `llm_output`, `tool_call`, `tool_result`, `final`, `error`

### 3.3 Enable tools on demand
```
curl -X POST http://127.0.0.1:18000/wunder ^
  -H "Content-Type: application/json" ^
  -H "X-API-Key: <your-api-key>" ^
  -d "{\"user_id\":\"u001\",\"question\":\"List current directory\",\"tool_names\":[\"列出文件\"],\"stream\":false}"
```

Fetch the tool catalog first:
```
curl -X GET http://127.0.0.1:18000/wunder/tools ^
  -H "X-API-Key: <your-api-key>"
```

## 4. API Entry Overview
See `docs/API文档.en.md` for details.

Core endpoints:
- `POST /wunder`
- `POST /wunder/system_prompt`
- `GET /wunder/tools`
- `GET /wunder/i18n`

Admin & ops:
- `GET/POST /wunder/admin/llm`
- `GET/POST /wunder/admin/mcp`
- `POST /wunder/admin/mcp/tools`
- `GET/POST /wunder/admin/skills`
- `POST /wunder/admin/skills/upload`
- `GET/POST /wunder/admin/tools`
- `GET /wunder/admin/monitor`
- `GET /wunder/admin/monitor/{session_id}`
- `POST /wunder/admin/monitor/{session_id}/cancel`

Workspace management:
- `GET /wunder/workspace`
- `POST /wunder/workspace/upload`
- `GET /wunder/workspace/download`
- `DELETE /wunder/workspace`

## 5. Workspace & History
- Workspace path: `workspaces/{user_id}` (prompt uses `/workspaces/{user_id}/` as the working directory)
- History: `data/historys/{user_id}/chat_history.jsonl`
- Tool logs: `data/historys/{user_id}/tool_log.jsonl`

Concurrent requests for the same `user_id` are rejected (HTTP 429).

## 6. Skills & MCP
### 6.1 Skills
- Skills are loaded from `EVA_SKILLS/` by default.
- Each skill folder must include `SKILL.md` with `name`, `description`, and `input_schema` in YAML frontmatter.
- Enable via `config/wunder.yaml` or `/wunder/admin/skills`.

### 6.2 MCP
- Configure `mcp.servers` in `config/wunder.yaml`.
- Manage via `/wunder/admin/mcp`.
- `/wunder/admin/mcp/tools` can probe and cache tool lists.

## 7. Configuration
Copy example config: `config/wunder-example.yaml` -> `config/wunder.yaml`
Base config: `config/wunder.yaml`
Persistent overrides: `data/config/wunder.override.yaml` (admin changes are written here)
LLM/MCP/tools are recommended to be configured via the admin console and saved to the override file.
- `server`: service port and stream chunk size
- `i18n`: default and supported languages
- `llm`: model service configuration
- `mcp`: MCP service configuration
- `skills`: skill paths and enabled list
- `tools`: enabled built-in tools
- `workspace`: workspace and retention policy
- `security`: command allowlist and path denylist
- `observability`: log level and path
- `cors`: CORS policy

## 8. Tests
See `docs/测试方案.en.md` for a full test plan.

## 9. Project Structure
```
app/                 # FastAPI entry and core logic
  api/               # routes and APIs
  orchestrator/      # orchestration engine and prompt builder
  tools/             # built-in tools and MCP adapter
  skills/            # Skills loading and registry
  memory/            # workspace and history
  monitor/           # monitoring and session state
config/              # configuration files
data/                # workspaces, history, logs
docs/                # design/API/test docs
web/                 # debug console static assets
tests/               # functional and load tests
```

## 10. Related Docs
- Design: `docs/设计方案.en.md`
- API: `docs/API文档.en.md`
- Test plan: `docs/测试方案.en.md`
- System overview: `docs/系统介绍.en.md`
- Request samples: `docs/请求示例.en.md`
- Narrative outline: `docs/叙事主线.en.md`
- PPT (EN): `/wunder/ppt-en`
