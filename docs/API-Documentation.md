# wunder API Documentation

## 4. API Design

### 4.0 Implementation Notes

- Routes are split by `src/api` modules (core/admin/workspace/user_tools/a2a) and share unified builders in Rust.
- Tool list and prompt injection share the same tool spec builder to stay consistent.
- Startup optimization: MCP, monitor, and orchestrator are lazily initialized; first calls may have cold-start delay.
- Lightweight entry: use `uvicorn app.asgi:app`; control warmup via `WUNDER_LAZY_WARMUP_S`.
- Config layering: base `config/wunder.yaml` (`WUNDER_CONFIG_PATH` to override); admin updates go to `data/config/wunder.override.yaml` (`WUNDER_CONFIG_OVERRIDE_PATH`).
- Auth: all `/wunder` and `/wunder/mcp` require `X-API-Key` or `Authorization: Bearer <key>`; key in `security.api_key`.
- Default admin account is `admin/admin`, auto-created on startup and protected from deletion.
- Registered users have daily request quotas (tiered by access level), reset at midnight; each model call consumes one unit and overages return 429. Virtual user IDs are not quota-limited.
- i18n: send `X-Wunder-Language` or `Accept-Language` (also `lang`/`language` query). Supported languages come from `i18n.supported_languages`. Responses include `Content-Language`, and system prompts/messages follow it.

### 4.1 `/wunder` Request

- Method: `POST`
- Body (JSON):
  - `user_id`: string, user identifier
  - `question`: string, user question
  - `tool_names`: list of strings, optional, tools to enable
  - `skip_tool_calls`: boolean, optional, ignore tool calls in model output and stop directly (default false)
  - `stream`: boolean, optional, stream response (default true)
  - `session_id`: string, optional, session id
  - `model_name`: string, optional, model config name
  - `config_overrides`: object, optional, per-request config overrides
  - `attachments`: list, optional, attachments (Markdown files or data URL images)
- Constraints: if a user already has a running session, returns 429.
- Constraints: registered users are quota-limited per model call; overages return 429 with `detail.code=USER_QUOTA_EXCEEDED`.
- Global concurrency cap: `server.max_active_sessions`.

### 4.1.1 `/wunder/system_prompt`

- Method: `POST`
- Body (JSON):
  - `user_id`: string
  - `session_id`: string, optional
  - `tool_names`: list, optional
  - `config_overrides`: object, optional
- Response:
  - `prompt`: string
  - `build_time_ms`: number

### 4.1.2 `/wunder/tools`

- Method: `GET`
- Query:
  - `user_id`: string, optional
- Response:
  - `builtin_tools`: built-in list (name/description/input_schema)
  - `mcp_tools`: MCP list (name/description/input_schema)
  - `skills`: skills list (name/description/input_schema)
  - `knowledge_tools`: knowledge list (name/description/input_schema)
  - `user_tools`: custom tools list
  - `shared_tools`: shared tools list (name/description/input_schema/owner_id)
  - `extra_prompt`: extra prompt text
- Notes:
  - Custom/shared tools are named as `user_id@tool` (MCP: `user_id@server@tool`).

### 4.1.2.1 `/wunder/user_tools/mcp`

- Method: `GET/POST`
- `GET` Query:
  - `user_id`: string
- `GET` Response:
  - `servers`: list of MCP services (name/endpoint/allow_tools/shared_tools/enabled/transport/description/display_name/headers/auth/tool_specs)
- `POST` Body:
  - `user_id`: string
  - `servers`: list of MCP services
- `POST` Response: same as `GET`

### 4.1.2.2 `/wunder/user_tools/mcp/tools`

- Method: `POST`
- Body:
  - `name`: service name
  - `endpoint`: endpoint
  - `transport`: optional
  - `headers`: optional
  - `auth`: optional
- Response:
  - `tools`: MCP tool list

### 4.1.2.3 `/wunder/user_tools/skills`

- Method: `GET/POST`
- `GET` Query:
  - `user_id`: string
- `GET` Response:
  - `enabled`: enabled skill names
  - `shared`: shared skill names
  - `skills`: list (name/description/path/input_schema/enabled/shared)
- `POST` Body:
  - `user_id`: string
  - `enabled`: list
  - `shared`: list
- `POST` Response: same as `GET`

### 4.1.2.4 `/wunder/user_tools/skills/content`

- Method: `GET`
- Query:
  - `user_id`: string
  - `name`: skill name
- Response:
  - `name`, `path`, `content`

### 4.1.2.5 `/wunder/user_tools/skills/upload`

- Method: `POST`
- Body (multipart/form-data):
  - `file`: skill zip
- Response:
  - `ok`, `extracted`, `message`

### 4.1.2.6 `/wunder/user_tools/knowledge`

- Method: `GET/POST`
- `GET` Query:
  - `user_id`: string
- `GET` Response:
  - `knowledge.bases`: list (name/description/root/enabled/shared)
- `POST` Body:
  - `user_id`: string
  - `knowledge.bases`: list (name/description/enabled/shared; root fixed by system)
- `POST` Response: same as `GET`

### 4.1.2.7 `/wunder/user_tools/knowledge/files`

- Method: `GET`
- Query:
  - `user_id`: string
  - `base`: knowledge base name
- Response:
  - `base`, `files`

### 4.1.2.8 `/wunder/user_tools/knowledge/file`

- Method: `GET/PUT/DELETE`
- `GET` Query:
  - `user_id`, `base`, `path`
- `GET` Response:
  - `base`, `path`, `content`
- `PUT` Body:
  - `user_id`, `base`, `path`, `content`
- `PUT` Response:
  - `ok`, `message`
- `DELETE` Query:
  - `user_id`, `base`, `path`
- `DELETE` Response:
  - `ok`, `message`

### 4.1.2.9 `/wunder/user_tools/knowledge/upload`

- Method: `POST`
- Body (multipart/form-data):
  - `user_id`, `base`, `file`
- Response:
  - `ok`, `message`, `path`, `converter`, `warnings`
- Note: this endpoint currently stores Markdown files only. For non-Markdown files, call `/wunder/doc2md/convert` first and upload the converted Markdown.

### 4.1.2.10 `/wunder/user_tools/extra_prompt`

- Method: `POST`
- Body:
  - `user_id`, `extra_prompt`
- Response:
  - `user_id`, `extra_prompt`

### 4.1.2.11 `/wunder/doc2md/convert`

- Method: `POST`
- Body (multipart/form-data):
  - `file`: file to parse
- Response:
  - `ok`, `name`, `content`, `converter`, `warnings`
- Note: no auth required; supports doc2md extensions. Internal attachment conversion uses the same logic.

### 4.1.2.12 `/wunder/attachments/convert`

- Method: `POST`
- Body (multipart/form-data):
  - `file`: file to parse
- Response:
  - `ok`, `name`, `content`, `converter`, `warnings`
- Note: debug UI only (auth required); conversion logic matches `/wunder/doc2md/convert`.

### 4.1.2.13 `/wunder/temp_dir/download`

- Method: `GET`
- Auth: none
- Query: `filename` (relative path under `temp_dir/`, no `..`)
- Note: downloads files from the project root `temp_dir/` folder.
- Response: file stream (`Content-Disposition: attachment`)

### 4.1.2.14 `/wunder/temp_dir/upload`

- Method: `POST`
- Auth: none
- Type: `multipart/form-data`
- Fields:
  - `file` (supports multiple files with the same field name)
  - `path` (optional subdir path under `temp_dir/`)
  - `overwrite` (optional, default true)
- Note: uploads files into the project root `temp_dir/`, creates subdir if `path` is set.
- Response:
  - `ok`
  - `files`

### 4.1.2.15 `/wunder/temp_dir/list`

- Method: `GET`
- Auth: none
- Note: list files under the project root `temp_dir/` (recursive, returns relative paths).
- Response:
  - `ok`
  - `files` (`name`/`size`/`updated_time`)

### 4.1.2.16 `/wunder/temp_dir/remove`

- Method: `POST`
- Auth: none
- Body (JSON):
  - `all`: clear all files when true
  - `filename`: relative path under `temp_dir/`
  - `filenames`: list of relative paths
- Response:
  - `ok`
  - `removed`
  - `missing`

### 4.1.2.17 `/wunder/mcp`

- Type: MCP service (streamable-http)
- Auth: `X-API-Key` or `Authorization: Bearer <key>`
- Tool: `wunder@excute`
  - Input: `task` string
  - Behavior: fixed `user_id = wunder`, uses enabled tools, filters `wunder@excute` itself
  - Output: `answer`/`session_id`/`usage`
- Tool: `wunder@doc2md`
  - Input: `source_url` (download URL, must include extension)
  - Behavior: download the file from `source_url` and convert to Markdown
  - Output: `name`/`content`/`converter`/`warnings`
- Endpoint config: `${WUNDER_MCP_ENDPOINT:-http://127.0.0.1:18000/wunder/mcp}`
- Timeout: `config.mcp.timeout_s`

### 4.1.2.18 `/wunder/i18n`

- Method: `GET`
- Response:
  - `default_language`
  - `supported_languages`
  - `aliases`

### 4.1.3 `/wunder/admin/mcp`

- Method: `GET/POST`
- `GET` Response: MCP server list
- `POST` Body: full server list to save

### 4.1.4 `/wunder/admin/mcp/tools`

- Method: `POST`
- Body: `name`, `endpoint`
- Response: `tools` list

#### `/wunder/admin/mcp/tools/call`

- Method: `POST`
- Body: `server`, `tool`, `args` (optional)
- Response: `result`, `warning` (optional)

### 4.1.5 `/wunder/admin/skills`

- Method: `GET/POST/DELETE`
- `GET` Response: `paths`, `enabled`, `skills`
- `POST` Body: `enabled`, `paths` (optional)
- `DELETE` Query: `name`
- Note: only skills under `EVA_SKILLS` can be deleted.

### 4.1.5.1 `/wunder/admin/skills/content`

- Method: `GET`
- Query: `name`
- Response: `name`, `path`, `content`

### 4.1.5.2 `/wunder/admin/skills/files`

- Method: `GET`
- Query: `name`
- Response: `name`, `root`, `entries` (`path`, `kind` as `dir/file`)

### 4.1.5.3 `/wunder/admin/skills/file`

- Method: `GET/PUT`
- `GET` Query: `name`, `path` (relative to skill root)
- `GET` Response: `name`, `path`, `content`
- `PUT` Body: `name`, `path`, `content`
- `PUT` Response: `ok`, `path`, `reloaded` (true when SKILL.md triggers reload)

### 4.1.6 `/wunder/admin/llm`

- Method: `GET/POST`
- `GET` Response: `llm.default`, `llm.models`
- `POST` Body: full LLM config

### 4.1.6.1 `/wunder/admin/llm/context_window`

- Method: `POST`
- Body: `provider`, `base_url`, `api_key`, `model`, `timeout_s`
- Response: `max_context`, `message`

### 4.1.6.2 `/wunder/admin/server`

- Method: `GET/POST`
- `GET` Response: `server.max_active_sessions`
- `POST` Body: `max_active_sessions` (> 0)
- `POST` Response: `server.max_active_sessions`

### 4.1.6.3 `/wunder/admin/security`

- Method: `GET`
- Response: `security.api_key` (null when not configured)
- Notes: admin-only, used by the console to prefill the default API key.

### 4.1.7 `/wunder/admin/skills/upload`

- Method: `POST`
- Body: `file` (zip)
- Response: `ok`, `extracted`

### 4.1.8 `/wunder/admin/monitor`

- Method: `GET`
- Query: `active_only`, `tool_hours`, `start_time`, `end_time`
- Notes: `start_time`/`end_time` overrides `tool_hours`; service/sandbox/tool_stats are window-scoped.
- Response: `system`, `service`, `sandbox`, `sessions`, `tool_stats`
  - `service`: active_sessions/history_sessions/finished_sessions/error_sessions/cancelled_sessions/total_sessions/avg_token_usage/avg_elapsed_s/avg_prefill_speed_tps/avg_decode_speed_tps
  - `sandbox`: mode/network/readonly_rootfs/idle_ttl_s/timeout_s/endpoint/image/resources(cpu/memory_mb/pids)/recent_calls/recent_sessions

### 4.1.8.1 `/wunder/admin/monitor/tool_usage`

- Method: `GET`
- Query: `tool`, `tool_hours`, `start_time`, `end_time`
- Response: `tool`, `sessions`

### 4.1.9 `/wunder/admin/monitor/{session_id}`

- Method: `GET`
- Response: `session`, `events`
- Notes: events are trimmed by `observability.monitor_event_limit` and `monitor_payload_max_chars` (<= 0 disables trimming).

### 4.1.10 `/wunder/admin/monitor/{session_id}/cancel`

- Method: `POST`
- Response: `ok`, `message`

### 4.1.11 `/wunder/admin/monitor/{session_id}`

- Method: `DELETE`
- Response: `ok`, `message`

### 4.1.12 `/wunder/workspace`

- Method: `GET`
- Query: `user_id`, `path`, `refresh_tree`, `keyword`, `offset`, `limit`, `sort_by`, `order`
- Response: `user_id`, `path`, `parent`, `entries`, `tree_version`, `total`, `offset`, `limit`

### 4.1.13 `/wunder/workspace/content`

- Method: `GET`
- Query: `user_id`, `path`, `include_content`, `max_bytes`, `depth`, `keyword`, `offset`, `limit`, `sort_by`, `order`
- Response: `user_id`, `path`, `type`, `size`, `updated_time`, `content`, `format`, `truncated`, `entries`, `total`, `offset`, `limit`

### 4.1.14 `/wunder/workspace/search`

- Method: `GET`
- Query: `user_id`, `keyword`, `offset`, `limit`, `include_files`, `include_dirs`
- Response: `user_id`, `keyword`, `entries`, `total`, `offset`, `limit`

### 4.1.15 `/wunder/workspace/upload`

- Method: `POST`
- Body (multipart/form-data): `user_id`, `path`, `files`, `relative_paths`
- Response: `ok`, `message`, `files`, `tree_version`

### 4.1.16 `/wunder/workspace/download`

- Method: `GET`
- Query: `user_id`, `path`
- Response: file stream

### 4.1.17 `/wunder/workspace/archive`

- Method: `GET`
- Query: `user_id`, `path` (optional)
- Response: zip stream

### 4.1.18 `/wunder/workspace`

- Method: `DELETE`
- Query: `user_id`, `path`
- Response: `ok`, `message`, `tree_version`

### 4.1.19 `/wunder/workspace/dir`

- Method: `POST`
- Body: `user_id`, `path`
- Response: `ok`, `message`, `tree_version`, `files`

### 4.1.20 `/wunder/workspace/move`

- Method: `POST`
- Body: `user_id`, `source`, `destination`
- Response: `ok`, `message`, `tree_version`, `files`

### 4.1.21 `/wunder/workspace/copy`

- Method: `POST`
- Body: `user_id`, `source`, `destination`
- Response: `ok`, `message`, `tree_version`, `files`

### 4.1.22 `/wunder/workspace/batch`

- Method: `POST`
- Body: `user_id`, `action`, `paths`, `destination`
- Response: `ok`, `message`, `tree_version`, `succeeded`, `failed`

### 4.1.23 `/wunder/workspace/file`

- Method: `POST`
- Body: `user_id`, `path`, `content`, `create_if_missing`
- Response: `ok`, `message`, `tree_version`, `files`

### 4.1.24.0 `/`

- Method: `GET`
- Description: admin debug UI entry (`web/index.html`); `web/simple-chat` is temporarily disabled.

### 4.1.24.1 `/wunder/ppt`

- Method: `GET`
- Description: system intro PPT (`docs/ppt`).

### 4.1.24.2 `/wunder/ppt-en`

- Method: `GET`
- Description: system intro PPT (EN, `docs/ppt-en`).

### 4.1.25 `/wunder/admin/tools`

- Method: `GET/POST`
- `GET` Response: `enabled`, `tools`
- `POST` Body: `enabled`

### 4.1.26 `/wunder/admin/knowledge`

- Method: `GET/POST`
- `GET` Response: `knowledge.bases`
- `POST` Body: `knowledge` (root auto-created if empty)

### 4.1.27 `/wunder/admin/knowledge/files`

- Method: `GET`
- Query: `base`
- Response: `base`, `files`

### 4.1.28 `/wunder/admin/knowledge/file`

- Method: `GET/PUT/DELETE`
- Query/Body: `base`, `path`, `content`

### 4.1.29 `/wunder/admin/knowledge/upload`

- Method: `POST`
- Body: `base`, `file`
- Response: `ok`, `message`, `path`, `converter`, `warnings`
- Note: this endpoint currently stores Markdown files only. For non-Markdown files, call `/wunder/doc2md/convert` first and upload the converted Markdown.

### 4.1.30 `/wunder/admin/knowledge/refresh`

- Method: `POST`
- Query: `base` (optional)
- Response: `ok`, `message`

### 4.1.31 `/wunder/admin/users`

- Method: `GET`
- Response: `users` (user_id, active_sessions, history_sessions, total_sessions, chat_records, tool_calls, token_usage)

### 4.1.32 `/wunder/admin/users/{user_id}/sessions`

- Method: `GET`
- Query: `active_only`
- Response: `user_id`, `sessions`

### 4.1.33 `/wunder/admin/users/{user_id}`

- Method: `DELETE`
- Response: `ok`, `message`, deleted counts, workspace_deleted, legacy_history_deleted

### 4.1.34 `/wunder/admin/memory/users`

- Method: `GET`
- Response: `users` (user_id, enabled, record_count, last_updated_time, last_updated_time_ts)

### 4.1.35 `/wunder/admin/memory/status`

- Method: `GET`
- Response: `active`, `history` queues

### 4.1.36 `/wunder/admin/memory/status/{task_id}`

- Method: `GET`
- Response: task detail (request/result/error)

### 4.1.37 `/wunder/admin/memory/{user_id}`

- Method: `GET`
- Response: `user_id`, `enabled`, `records`

### 4.1.38 `/wunder/admin/memory/{user_id}/{session_id}`

- Method: `PUT`
- Body: `summary`
- Response: `ok`, `message`

### 4.1.39 `/wunder/admin/memory/{user_id}/enabled`

- Method: `POST`
- Body: `enabled`
- Response: `user_id`, `enabled`

### 4.1.40 `/wunder/admin/memory/{user_id}/{session_id}`

- Method: `DELETE`
- Response: `ok`, `message`, `deleted`

### 4.1.41 `/wunder/admin/memory/{user_id}`

- Method: `DELETE`
- Response: `ok`, `message`, `deleted`

## 4.2 Streaming (SSE)

- Content type: `text/event-stream`
- Round fields: `data.user_round` (user turn), `data.model_round` (model turn).
- `event: progress`: progress summary
- `event: llm_request`: model request payload (debug)
- `event: knowledge_request`: knowledge request payload (debug)
- `event: llm_output_delta`: stream delta (`data.delta`, `data.reasoning_delta`)
- `event: llm_stream_retry`: retry info
- `event: llm_output`: final aggregated output
- `event: token_usage`: token usage per model round (includes `user_round/model_round`)
- `event: quota_usage`: quota consumption per model call (`daily_quota/used/remaining/date`, `consumed`, includes `user_round/model_round`)
- `event: tool_call`: tool call info
- `event: tool_output_delta`: tool output streaming chunk (`data.tool`/`data.command`/`data.stream`/`data.delta`; currently only for local built-in `execute_command`, not sandboxed)
- `event: tool_result`: tool execution result
- `event: compaction`: context compaction info
- `event: final`: final response
- `event: error`: error info
- Each SSE event includes an `id` for ordering.
- Overflowed events are stored in `stream_events` and replayed.

Example payload:
```json
{
  "type": "progress",
  "timestamp": "2025-12-24T08:30:00Z",
  "session_id": "u_1234_20251224",
  "data": {
    "stage": "plan",
    "summary": "Completed requirement breakdown, preparing tool calls."
  }
}
```

## 4.3 Non-stream response

- JSON:
  - `session_id`
  - `answer`
  - `usage` (optional)

## 4.4 Tool protocol (EVA style)

- `tool_call_mode=tool_call` (default): model wraps tool calls in `<tool_call>...</tool_call>`, tool results are returned as `tool_response: ` user messages.
- `tool_call_mode=function_call`: model returns OpenAI-style `tool_calls/function_call`, tool results are returned as role="tool" messages with tool_call_id.
- JSON: `{"name":"tool","arguments":{...}}`.
- Tool results are returned as a user message prefixed with `tool_response: ` (`tool_call` mode).
- Command allowlist controlled by `security.allow_commands`.
- `workdir` sets working dir (relative to workspace only).
- File tools are restricted to workspace unless in `security.allow_paths`.
- MCP tool name format: `server@tool`; Skills: enabled skill names.

Example:
```text
<tool_call>
{"name":"列出文件","arguments":{"path":"."}}
</tool_call>
```

## 4.5 Storage

- Storage backend is configurable via `storage.backend`: `auto` (default, prefer PostgreSQL and fall back to SQLite), `postgres`, or `sqlite`.
- PostgreSQL config uses `storage.postgres.dsn` (env: `WUNDER_POSTGRES_DSN`) and `storage.postgres.connect_timeout_s` (supports `${VAR:-default}` placeholders in YAML strings).
- SQLite uses `storage.db_path`.
- System logs, chat history, tool logs, artifacts, monitor records, session locks, and overflow events are stored in the selected backend.
- Legacy `data/historys/` is kept for migration only.

## 4.6 Sandbox service API

> Note: the shared sandbox service is provided by a second wunder container.

### 4.6.1 `GET /health`

- Response: `ok` boolean

### 4.6.2 `POST /sandboxes/execute_tool`

- Body:
  - `user_id`, `session_id`, `tool`, `args`, `workspace_root`, `allow_paths`, `deny_globs`, `allow_commands`, `container_root`, `image`, `network`, `readonly_rootfs`, `idle_ttl_s`, `resources`
- Response:
  - `ok`, `data`, `error`, `debug_events`

### 4.6.3 `POST /sandboxes/release`

- Body: `user_id`, `session_id`
- Response: `ok`, `message`

### 4.6.4 Notes

- Shared sandbox does not create child containers; it relies on the same image and workspace mounts.
- For docker compose deployments, prefer internal DNS `http://sandbox:9001` (no published port 9001). At runtime, `WUNDER_SANDBOX_ENDPOINT` is preferred and the client falls back between common `sandbox`/`127.0.0.1` endpoints to reduce IP-related failures.

## 5. Appendix: helper scripts

- `scripts/update_feature_log.py`: write categorized entries to `docs/功能迭代.md` (supports `--type/--scope`), with UTF-8 BOM to avoid encoding issues.
