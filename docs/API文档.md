# wunder API 文档

## 4. API 设计

### 4.0 实现说明

- 接口实现基于 Rust Axum，路由拆分在 `src/api` 的 core/admin/workspace/user_tools/a2a 模块。
- 运行与热重载环境建议使用 `Dockerfile` + `docker-compose-x86.yml`/`docker-compose-arm.yml`。
- MCP 服务容器：`wunder_mcp` 用于运行 `mcp_server/` 下的 FastMCP 服务脚本，默认以 streamable-http 暴露端口，人员数据库连接通过 `mcp_server/mcp_config.json` 的 `database` 配置。
- MCP 配置文件：`mcp_server/mcp_config.json` 支持集中管理人员数据库配置，可通过 `MCP_CONFIG_PATH` 指定路径，数据库配置以配置文件为准。
- 多数据库支持：在 `mcp_config.json` 的 `database.targets` 中配置多个数据库（MySQL/PostgreSQL），默认使用 `default_key`，需要切换目标可调整 `default_key` 或部署多个 MCP 实例。
- Database query tools: configure `database.tables` (or `database.query_tables`) to auto-register table-scoped `db_query` tools (`db_query` for single table, `db_query_<key>` for multiple). Each tool is hard-bound to its table and embeds compact schema hints (`column + type`) in description.
- 单库类型切换：设置 `database.db_type=mysql|postgres`，或在多库配置中为每个目标指定 `type/engine` 或 DSN scheme。
- 知识库 MCP：按 `knowledge.targets` 动态注册 `kb_query` 工具（单目标为 `kb_query`，多目标自动命名为 `kb_query_<key>`）；向量知识库检索不依赖 RAGFlow MCP。
- 向量知识库使用 Weaviate，连接参数位于 `vector_store.weaviate`（url/api_key/timeout_s/batch_size）。
- docker compose 默认使用命名卷 `wunder_postgres` 保存 PostgreSQL 数据，避免绑定到 `data/` 目录。
- 沙盒服务：独立容器运行 `wunder-server` 的 `sandbox` 模式（`WUNDER_SERVER_MODE=sandbox`），对外提供 `/sandboxes/execute_tool` 与 `/sandboxes/release`，由 `WUNDER_SANDBOX_ENDPOINT` 指定地址。
- 工具清单与提示词注入复用统一的工具规格构建逻辑，确保输出一致性（`tool_call` 模式）；`function_call` 模式不注入工具提示词，工具清单仅用于 tools 协议。
- 配置分层：基础配置为 `config/wunder.yaml`（`WUNDER_CONFIG_PATH` 可覆盖），管理端修改会写入 `data/config/wunder.override.yaml`（`WUNDER_CONFIG_OVERRIDE_PATH` 可覆盖）。
- 环境变量：建议使用仓库根目录 `.env` 统一管理常用变量，docker compose 默认读取（如 `WUNDER_HOST`/`WUNDER_PORT`/`WUNDER_API_KEY`/`WUNDER_POSTGRES_DSN`/`WUNDER_SANDBOX_ENDPOINT`）。
- 前端入口：管理端调试 UI `http://127.0.0.1:18000`，调试前端 `http://127.0.0.1:18001`（Vite dev server），用户侧前端 `http://127.0.0.1:18002`（Nginx 静态服务）。
- 鉴权：管理员接口使用 `X-API-Key` 或 `Authorization: Bearer <api_key>`（配置项 `security.api_key`），用户侧接口使用 `/wunder/auth` 颁发的 `Authorization: Bearer <user_token>`。
- 默认管理员账号为 admin/admin，服务启动时自动创建且不可删除，可通过用户管理重置密码。
- 用户端请求可省略 `user_id`，后端从 Token 解析；管理员接口可显式传 `user_id` 以指定目标用户。
- 模型配置新增 `model_type=llm|embedding`，向量知识库依赖 embedding 模型调用 `/v1/embeddings`。
- User frontend default entry is `/app/chat`; world page entry is `/home` (actual route `/app/home`); external app detail route is `/app/external/:linkId` (demo route `/demo/external/:linkId`). External links are managed via `/wunder/admin/external_links` and delivered by `/wunder/external_links` after org-level filtering; production frontend port is 18002, development port is 18001.
- 当使用 API Key/管理员 Token 访问 `/wunder`、`/wunder/chat`、`/wunder/workspace`、`/wunder/user_tools` 时，`user_id` 允许为“虚拟用户”，无需在 `user_accounts` 注册，仅用于线程/工作区/工具隔离。
- 注册用户按单位层级分配默认每日额度（一级/二级/三级/四级 = 10000/5000/1000/100），每日 0 点重置；额度按每次模型调用消耗，超额返回 429，虚拟用户不受限制。
- 管理员用户执行请求不受额度、会话锁、历史裁剪、监控裁剪、模型/工具超时与历史清理限制，适合长期运行任务。
- A2A 接口：`/a2a` 提供 JSON-RPC 2.0 绑定，`SendStreamingMessage` 以 SSE 形式返回流式事件，AgentCard 通过 `/.well-known/agent-card.json` 暴露。
- 多语言：Rust 版默认从 `config/i18n.messages.json` 读取翻译（可用 `WUNDER_I18N_MESSAGES_PATH` 覆盖）；`/wunder/i18n` 提供语言配置，响应包含 `Content-Language`。
- Rust 版现状：MCP 服务与工具发现/调用已落地（rmcp + streamable-http）；Skills/知识库转换与数据库持久化仍在迁移，相关接口以轻量结构返回。

### 4.0.1 统一错误响应（HTTP）

- HTTP 错误统一返回 JSON 结构：
  - `ok`：固定为 `false`
  - `error.code`：稳定错误码（例如 `BAD_REQUEST` / `UNAUTHORIZED` / `NOT_FOUND` / `INTERNAL_ERROR`）
  - `error.message`：人类可读错误信息
  - `error.status`：HTTP 状态码数值
  - `error.hint`：可执行的排障提示
  - `error.trace_id`：请求级追踪 ID，同时通过响应头 `x-trace-id` 返回
  - `error.timestamp`：UNIX 时间戳（秒，浮点）
- 兼容历史客户端（4.0.1 之前依赖旧格式）时仍保留 `detail.message`。
- 响应头同步返回：
  - `x-trace-id`：与 `error.trace_id` 一致
  - `x-error-code`：与 `error.code` 一致

### 4.1 `/wunder` 请求

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：字符串，用户唯一标识
  - `question`：字符串，用户问题
  - `tool_names`：字符串列表，可选，指定启用的内置工具/MCP/技能名称
  - `skip_tool_calls`：布尔，可选，是否忽略模型输出中的工具调用并直接结束（默认 false）
  - `stream`：布尔，可选，是否流式输出（默认 true）
  - `debug_payload`：布尔，可选，调试用；仅管理员调试会话（`is_admin=true`）开启后会保留模型请求体用于事件与日志记录（默认 false）
  - `session_id`：字符串，可选，指定会话标识
  - `agent_id`：字符串，可选，智能体应用 id（用于附加提示词与沙盒容器工作区路由）
  - `model_name`：字符串，可选，模型配置名称（不传则使用默认模型）
- `config_overrides`：对象，可选，用于临时覆盖配置
- `attachments`：数组，可选，附件列表（文件为 Markdown 文本，图片为 data URL）
- 约束：注册用户每日有请求额度，按每次模型调用消耗，超额返回 429（`detail.code=USER_QUOTA_EXCEEDED`）。
- 忙时队列：当 `agent_queue.enabled=true` 时，非流式返回 202（`data.queue_id`/`data.thread_id`/`data.session_id`），SSE/WS 返回 `queued` 事件。
- 忙时返回：当 `agent_queue.enabled=false` 且显式指定 `session_id` 正在运行/取消中时，会返回 429（`detail.code=USER_BUSY`）。
- 说明：未传 `session_id` 且主会话正忙时，会自动分叉独立会话继续处理，并返回新的 `session_id`（不覆盖主会话）。
- 说明：问询面板进入 `waiting` 后，用户选择路线会被当作正常请求立即继续处理，不会被判定为“会话繁忙”进入队列。
- 约束：全局并发上限由 `server.max_active_sessions` 控制，超过上限的请求会排队等待。
- 说明：管理员会话跳过上述限制（会话锁/额度/并发上限）。
- 说明：当 `tool_names` 显式包含 `a2ui` 时，系统会剔除“最终回复”工具并改为输出 A2UI 消息；SSE 将追加 `a2ui` 事件，非流式响应会携带 `uid`/`a2ui` 字段。
- 说明：`/wunder` 入口允许传入未注册的 `user_id`，作为线程标识与隔离空间使用。

### 4.1.1 `/wunder/system_prompt`

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：字符串，用户唯一标识
  - `session_id`：字符串，可选，会话标识
  - `tool_names`：字符串列表，可选，指定启用的内置工具/MCP/技能名称（内置工具支持英文别名）
  - `config_overrides`：对象，可选，用于临时覆盖配置
  - `agent_prompt`：字符串，可选，智能体追加提示词
- 返回（JSON）：
  - `prompt`：字符串，当前系统提示词
  - `build_time_ms`：数字，系统提示词构建耗时（毫秒）

### 4.1.2 `/wunder/tools`

- 方法：`GET`
- 入参（Query）：
- `user_id`：字符串，可选，用户唯一标识（传入后返回自建/共享工具）
- 返回（JSON）：
  - `builtin_tools`：内置工具列表（name/description/input_schema）
  - `mcp_tools`：MCP 工具列表（name/description/input_schema）
  - `a2a_tools`：A2A 服务工具列表（name/description/input_schema）
  - `skills`：技能列表（name/description/input_schema）
  - `knowledge_tools`：知识库工具列表（字面/向量，name/description/input_schema）
  - `user_tools`：自建工具列表（name/description/input_schema）
  - `shared_tools`：共享工具列表（name/description/input_schema/owner_id）
  - `shared_tools_selected`：共享工具勾选列表（可选）
- 说明：
  - 自建/共享工具名称统一为 `user_id@工具名`（MCP 为 `user_id@server@tool`）。
  - 知识库工具入参支持 `query` 或 `keywords` 列表（二选一），`limit` 可选；向量知识库会按关键词逐一检索并在结果中返回 `queries` 分组（多关键词时 `documents` 追加 `keyword`）。
- 内置工具名称同时提供英文别名（如 `read_file`、`write_file`），可用于接口选择与工具调用。
- 新增内置工具 `计划面板`（英文别名 `update_plan`），用于更新计划看板并触发 `plan_update` 事件。
- 新增内置工具 `问询面板`（英文别名 `question_panel`/`ask_panel`），用于提供多条路线选择并触发 `question_panel` 事件。
- 新增内置工具 `技能调用`（英文别名 `skill_call`/`skill_get`），传入技能名返回完整 SKILL.md 与技能目录结构。
- 新增内置工具 `子智能体控制`（英文别名 `subagent_control`），通过 `action=list|history|send|spawn` 统一完成会话列表/历史/发送/派生。
- 新增内置工具 `智能体蜂群`（英文别名 `agent_swarm`/`swarm_control`），通过 `action=list|status|send|history|spawn|batch_send|wait` 管理当前用户“当前智能体以外”的其他智能体。
- `智能体蜂群` 的 `send` 支持按 `agent_id` 自动复用会话，必要时可通过 `createIfMissing=true` 自动创建新会话，再发送指令。
- `智能体蜂群` 新增 `wait` 动作：可直接等待 `run_ids` 结果并返回聚合状态，避免母蜂反复轮询 `status`。
- 多工蜂协作推荐：先 `batch_send` 一次并发派发，再 `wait` 统一收敛。
- `子智能体控制` 的 `send` 支持 `timeoutSeconds` 等待回复，`spawn` 支持 `runTimeoutSeconds` 等待完成并返回 `reply/elapsed_s`。
- 新增内置工具 `节点调用`（英文别名 `node.invoke`/`node_invoke`），通过 `action=list|invoke` 统一完成节点发现与节点调用。
- `action=list` 返回当前在线节点清单（含 `node_id/commands/caps/scopes` 等信息）；`action=invoke` 需要 `node_id + command`，可选 `args/timeout_s/metadata`。
- 兼容旧入参：未传 `action` 但同时提供 `node_id + command` 时仍按 `invoke` 处理。
- A2A 服务工具命名为 `a2a@service`，服务由管理员配置并启用。
- 内置提供 `a2a观察`/`a2a等待`，用于观察任务状态与等待结果。

### 4.1.2.1 `/wunder/user_tools/mcp`

- 方法：`GET/POST`
- `GET` 入参（Query）：
  - `user_id`：字符串，用户唯一标识
- `GET` 返回（JSON）：
  - `servers`：用户 MCP 服务列表（name/endpoint/allow_tools/shared_tools/enabled/transport/description/display_name/headers/auth/tool_specs）
- `POST` 入参（JSON）：
  - `user_id`：用户唯一标识
  - `servers`：用户 MCP 服务列表（字段同上）
- `POST` 返回：同 `GET`

### 4.1.2.2 `/wunder/user_tools/mcp/tools`

- 方法：`POST`
- 入参（JSON）：
  - `name`：服务名称
  - `endpoint`：服务地址
  - `transport`：传输类型（可选）
  - `headers`：请求头对象（可选）
  - `auth`：认证字段（可选）
- 返回（JSON）：
  - `tools`：MCP 工具清单

### 4.1.2.3 `/wunder/user_tools/skills`

- 方法：`GET/POST/DELETE`
- `GET` 入参（Query）：
  - `user_id`：字符串，用户唯一标识
- `GET` 返回（JSON）：
  - `enabled`：已启用技能名列表
  - `shared`：已共享技能名列表
  - `skills`：技能列表（name/description/path/input_schema/enabled/shared）
- `POST` 入参（JSON）：
  - `user_id`：用户唯一标识
  - `enabled`：启用技能名列表
  - `shared`：共享技能名列表
- `POST` 返回：同 `GET`
- `DELETE` 入参（Query）：
  - `user_id`：用户唯一标识
  - `name`：技能名称
- `DELETE` 返回（JSON）：
  - `ok`：是否成功
  - `name`：技能名称
  - `message`：提示信息

### 4.1.2.4 `/wunder/user_tools/skills/files`

- 方法：`GET`
- 入参（Query）：
  - `user_id`：用户唯一标识
  - `name`：技能名称
- 返回（JSON）：
  - `name`：技能名称
  - `root`：技能根目录
  - `entries`：文件列表（path/kind）

### 4.1.2.5 `/wunder/user_tools/skills/file`

- 方法：`GET/PUT`
- `GET` 入参（Query）：
  - `user_id`：用户唯一标识
  - `name`：技能名称
  - `path`：相对技能目录的文件路径
- `GET` 返回（JSON）：
  - `name`：技能名称
  - `path`：文件路径
  - `content`：文件内容
- `PUT` 入参（JSON）：
  - `user_id`：用户唯一标识
  - `name`：技能名称
  - `path`：相对技能目录的文件路径
  - `content`：文件内容
- `PUT` 返回（JSON）：
  - `ok`：是否成功
  - `path`：文件路径
  - `reloaded`：是否触发技能刷新（编辑 SKILL.md 时为 true）

### 4.1.2.6 `/wunder/user_tools/skills/content`

- 方法：`GET`
- 入参（Query）：
  - `user_id`：用户唯一标识
  - `name`：技能名称
- 返回（JSON）：
  - `name`：技能名称
  - `path`：SKILL.md 文件路径
  - `content`：SKILL.md 完整内容

### 4.1.2.7 `/wunder/user_tools/skills/upload`

- 方法：`POST`
- 入参：`multipart/form-data`
  - `file`：技能 .zip 或 .skill 压缩包
- 返回（JSON）：
  - `ok`：是否成功
  - `extracted`：解压文件数量
  - `message`：提示信息

### 4.1.2.8 `/wunder/user_tools/knowledge`

- 方法：`GET/POST`
- `GET` 入参（Query）：
  - `user_id`：用户唯一标识
- `GET` 返回（JSON）：
  - `knowledge.bases`：知识库列表（name/description/root/enabled/shared/base_type/embedding_model/chunk_size/chunk_overlap/top_k/score_threshold）
  - `embedding_models`：可用嵌入模型名称列表（仅包含 model_type=embedding）
- `POST` 入参（JSON）：
  - `user_id`：用户唯一标识
  - `knowledge.bases`：知识库列表（name/description/enabled/shared/base_type/embedding_model/chunk_size/chunk_overlap/top_k/score_threshold）
- `POST` 返回：同 `GET`
- 说明：`base_type` 为空默认字面知识库；`base_type=vector` 时必须指定 `embedding_model`，root 自动指向 `vector_knowledge/users/<user_id>/<base>` 作为逻辑标识，向量文档与切片元数据存储在数据库中。

### 4.1.2.9 `/wunder/user_tools/knowledge/files`

- 方法：`GET`
- 入参（Query）：
  - `user_id`：用户唯一标识
  - `base`：知识库名称
- 返回（JSON）：
  - `base`：知识库名称
  - `files`：Markdown 文件相对路径列表
- 说明：仅适用于字面知识库，向量知识库请使用 `/wunder/user_tools/knowledge/docs` 等接口。

### 4.1.2.10 `/wunder/user_tools/knowledge/file`

- 方法：`GET/PUT/DELETE`
- `GET` 入参（Query）：
  - `user_id`：用户唯一标识
  - `base`：知识库名称
  - `path`：相对知识库根目录的文件路径
- `GET` 返回（JSON）：
  - `base`：知识库名称
  - `path`：文件路径
  - `content`：文件内容
- `PUT` 入参（JSON）：
  - `user_id`：用户唯一标识
  - `base`：知识库名称
  - `path`：文件路径
  - `content`：文件内容
- `PUT` 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
- `DELETE` 入参（Query）：
  - `user_id`：用户唯一标识
  - `base`：知识库名称
  - `path`：文件路径
- `DELETE` 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
- 说明：仅适用于字面知识库，向量知识库请使用 `/wunder/user_tools/knowledge/doc` 等接口。

### 4.1.2.11 `/wunder/user_tools/knowledge/upload`

- 方法：`POST`
- 入参（multipart/form-data）：
  - `user_id`：用户唯一标识
  - `base`：知识库名称
  - `file`：待上传文件
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
  - `path`：转换后的 Markdown 相对路径（字面知识库）
  - `doc_id`：向量文档 id（向量知识库）
  - `doc_name`：向量文档名称（向量知识库）
  - `chunk_count`：切片数量（向量知识库）
  - `embedding_model`：嵌入模型（向量知识库）
  - `converter`：使用的转换器（doc2md/text/html/code/pdf/raw）
  - `warnings`：转换警告列表
- 说明：该接口支持 doc2md 可解析的格式，上传后自动转换为 Markdown 保存，原始非 md 文件不会落库并会清理；向量知识库上传仅解析并切片，需通过 `/wunder/user_tools/knowledge/reindex` 生成向量。

### 4.1.2.12 `/wunder/user_tools/knowledge/docs`

- 方法：`GET`
- 入参（Query）：
  - `user_id`：用户唯一标识
  - `base`：知识库名称
- 返回（JSON）：
  - `base`：知识库名称
  - `docs`：向量文档列表（doc_id/name/status/chunk_count/embedding_model/updated_at）
- 说明：仅适用于向量知识库。

### 4.1.2.13 `/wunder/user_tools/knowledge/doc`

- 方法：`GET/DELETE`
- `GET` 入参（Query）：
  - `user_id`：用户唯一标识
  - `base`：知识库名称
  - `doc_id`：文档 id
- `GET` 返回（JSON）：
  - `base`：知识库名称
  - `doc`：文档元数据（embedding_model/chunk_size/chunk_overlap/chunk_count/status/updated_at/chunks[index/start/end/status/content]）
  - `content`：原文内容
- `DELETE` 入参（Query）：
  - `user_id`：用户唯一标识
  - `base`：知识库名称
  - `doc_id`：文档 id
- `DELETE` 返回（JSON）：
  - `ok`：是否成功
  - `deleted`：删除的向量条目数量
  - `doc_id`：文档 id
  - `doc_name`：文档名称
- 说明：仅适用于向量知识库。

### 4.1.2.14 `/wunder/user_tools/knowledge/chunks`

- 方法：`GET`
- 入参（Query）：
  - `user_id`：用户唯一标识
  - `base`：知识库名称
  - `doc_id`：文档 id
- 返回（JSON）：
  - `base`：知识库名称
  - `doc_id`：文档 id
  - `chunks`：切片列表（index/start/end/preview/content/status）
- 说明：仅适用于向量知识库。

### 4.1.2.15 `/wunder/user_tools/knowledge/chunk/embed`

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：用户唯一标识
  - `base`：知识库名称
  - `doc_id`：文档 id
  - `chunk_index`：切片序号
- 返回（JSON）：
  - `ok`：是否成功
  - `doc`：更新后的文档元数据
- 说明：仅适用于向量知识库。

### 4.1.2.16 `/wunder/user_tools/knowledge/chunk/delete`

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：用户唯一标识
  - `base`：知识库名称
  - `doc_id`：文档 id
  - `chunk_index`：切片序号
- 返回（JSON）：
  - `ok`：是否成功
  - `doc`：更新后的文档元数据
- 说明：仅适用于向量知识库。

### 4.1.2.17 `/wunder/user_tools/knowledge/test`

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：用户唯一标识
  - `base`：知识库名称
  - `query`：测试问题
  - `top_k`：召回数量（可选）
- 返回（JSON）：
  - `base`：知识库名称
  - `query`：测试问题
  - `embedding_model`：嵌入模型（向量知识库）
  - `top_k`：召回数量（向量知识库）
  - `hits`：召回列表（doc_id/document/chunk_index/start/end/content/embedding_model/score）
  - `text`：字面知识库结果文本
- 说明：支持向量/字面知识库。

### 4.1.2.18 `/wunder/user_tools/knowledge/reindex`

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：用户唯一标识
  - `base`：知识库名称
  - `doc_id`：文档 id（可选，留空则重建全部）
- 返回（JSON）：
  - `ok`：是否成功
  - `reindexed`：已重建的 doc_id 列表
  - `failed`：失败项列表（doc_id/error）
- 说明：仅适用于向量知识库。

### 4.1.2.19 `/wunder/user_tools/tools`

- 方法：`GET`
- 返回（JSON）：
  - `builtin_tools`：内置工具列表（name/description/input_schema）
  - `mcp_tools`：MCP 工具列表（name/description/input_schema）
  - `a2a_tools`：A2A 服务工具列表（name/description/input_schema）
  - `skills`：技能列表（name/description/input_schema）
- `knowledge_tools`：知识库工具列表（字面/向量，name/description/input_schema）
  - `user_tools`：自建工具列表（name/description/input_schema）
  - `shared_tools`：共享工具列表（name/description/input_schema/owner_id）
  - `shared_tools_selected`：共享工具勾选列表（字符串数组）
- 说明：返回的是当前用户实际可用工具（已按等级与共享勾选过滤）。
- 说明：知识库工具入参支持 `query` 或 `keywords` 列表（二选一），`limit` 可选。

### 4.1.2.20 `/wunder/user_tools/catalog`

- 方法：`GET`
- 返回（JSON）：
  - 字段同 `/wunder/user_tools/tools`
- 说明：用于工具管理页面，返回所有共享工具（不按勾选过滤）。

### 4.1.2.21 `/wunder/user_tools/shared_tools`

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：用户唯一标识（可选）
  - `shared_tools`：共享工具勾选列表（字符串数组）
- 返回（JSON）：
  - `user_id`：用户唯一标识
  - `shared_tools`：共享工具勾选列表

### 4.1.2.22 `/wunder/doc2md/convert`

- 方法：`POST`
- 入参：`multipart/form-data`
  - `file`：待解析文件（可传多个同名字段）
- 返回（JSON）：
  - `ok`：是否成功
  - 单文件：`name`/`content`/`converter`/`warnings`
  - 多文件：`items`（数组，元素包含 `name`/`content`/`converter`/`warnings`）
- 说明：接口无需鉴权，系统内部附件转换统一调用该逻辑。
- 支持扩展名：`.txt/.md/.markdown/.html/.htm/.py/.c/.cpp/.cc/.h/.hpp/.json/.js/.ts/.css/.ini/.cfg/.log/.doc/.docx/.odt/.pdf/.pptx/.odp/.xlsx/.ods/.wps/.et/.dps`。
- 上传限制：默认 200MB。

### 4.1.2.23 `/wunder/attachments/convert`

- 方法：`POST`
- 入参：`multipart/form-data`
  - `file`：待解析文件（可传多个同名字段）
- 返回（JSON）：
  - `ok`：是否成功
  - 单文件：`name`/`content`/`converter`/`warnings`
  - 多文件：`items`（数组，元素包含 `name`/`content`/`converter`/`warnings`）
- 说明：`/wunder/attachments/convert` 用于调试面板（需鉴权），解析逻辑与 `/wunder/doc2md/convert` 一致。

### 4.1.2.24 `/wunder/temp_dir/download`

- 方法：`GET`
- 鉴权：无
- 入参（query）：`filename` 文件路径（相对 `temp_dir/`，不支持 `..`）
- 说明：从项目根目录 `temp_dir/` 目录读取文件并下载。
- 返回：文件流（`Content-Disposition: attachment`）

### 4.1.2.25 `/wunder/temp_dir/upload`

- 方法：`POST`
- 鉴权：无
- 类型：`multipart/form-data`
- 入参：
  - `file` 文件字段（支持多个同名字段）
  - `path` 目标子目录路径（相对 `temp_dir/`，可选）
  - `overwrite` 是否覆盖同名文件（可选，默认 true）
- 说明：上传文件到项目根目录 `temp_dir/`，若设置 `path` 则自动创建目录。
- 返回（JSON）：
  - `ok`：是否成功
  - `files`：上传后的文件名列表

### 4.1.2.26 `/wunder/temp_dir/list`

- 方法：`GET`
- 鉴权：无
- 说明：列出项目根目录 `temp_dir/` 的文件（包含子目录，返回相对路径）。
- 返回（JSON）：
  - `ok`：是否成功
  - `files`：文件列表（`name`/`size`/`updated_time`）

### 4.1.2.27 `/wunder/temp_dir/remove`

- 方法：`POST`
- 鉴权：无
- 入参（JSON）：
  - `all`：是否清空目录（true 表示清空）
  - `filename`：要删除的文件路径（相对 `temp_dir/`）
  - `filenames`：要删除的文件路径数组（相对 `temp_dir/`）
- 返回（JSON）：
  - `ok`：是否成功
  - `removed`：已删除文件名列表
  - `missing`：未找到的文件名列表

### 4.1.2.28 `/wunder/mcp`

- 类型：MCP 服务（streamable-http）
- 说明：系统自托管 MCP 入口，默认在管理员 MCP 服务管理中内置但未启用。
- Rust 版已实现该入口，基于 rmcp 的 streamable-http 传输。
- 鉴权：请求头需携带 `X-API-Key` 或 `Authorization: Bearer <key>`。
- 工具：`excute`（在 wunder 内部映射为 `wunder@excute`）
  - 入参：`task` 字符串，任务描述
  - 行为：使用固定 `user_id = wunder` 执行任务，按管理员启用的工具清单运行，并剔除 `wunder@excute` 避免递归调用
  - 返回：`answer`/`session_id`/`usage`
- 工具：`doc2md`（在 wunder 内部映射为 `wunder@doc2md`）
  - 入参：`source_url` 文件下载地址（URL，需包含扩展名）
  - 行为：下载 `source_url` 对应文件后解析并返回 Markdown
  - 返回：`name`/`content`/`converter`/`warnings`
- 参考配置：`endpoint` 默认可设为 `${WUNDER_MCP_ENDPOINT:-http://127.0.0.1:18000/wunder/mcp}`
- 超时配置：MCP 调用全局超时由 `config.mcp.timeout_s` 控制（秒）

### 4.1.2.29 `/wunder/i18n`

- 方法：`GET`
- 返回（JSON）：
  - `default_language`：默认语言
  - `supported_languages`：支持语言列表
  - `aliases`：语言别名映射

### 4.1.2.30 `/wunder/cron/*`

- 说明：定时任务管理（用户侧）。
- `GET /wunder/cron/list`：列出当前用户的定时任务
  - 返回：`data.jobs`（包含 job_id/name/schedule/next_run_at/last_status 等）
- `GET /wunder/cron/runs?job_id=...&limit=...`：查询任务运行记录
  - 返回：`data.runs`
- `POST /wunder/cron/add|update|remove|enable|disable|get|run|action`：新增/更新/删除/启停/查询/立即执行
  - 入参：与内置工具 `schedule_task` schema 一致（`action` + `job`）
  - 说明：`job.schedule.kind=every` 时支持可选 `schedule.at` 作为首次触发时间锚点；若未提供则默认以任务创建时间为起点。
  - 返回：`data` 中包含 action 结果与 job 信息

### 4.1.3 `/wunder/admin/mcp`

- 方法：`GET/POST`
- `GET` 返回：
  - `servers`：MCP 服务列表（name/endpoint/allow_tools/enabled）
- `POST` 入参：
  - `servers`：完整 MCP 服务列表，用于保存配置

### 4.1.3.1 `/wunder/admin/lsp`

- 方法：`GET/POST`
- `GET` 返回：
  - `lsp`：LSP 配置（enabled/timeout_s/diagnostics_debounce_ms/idle_ttl_s/servers）
  - `status`：LSP 连接状态列表（server_id/server_name/user_id/root/status/last_used_at）
- `POST` 入参：
  - `lsp`：完整 LSP 配置，用于保存配置

#### `/wunder/admin/lsp/test`

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：用户唯一标识
  - `path`：文件路径（相对用户工作区）
  - `operation`：definition/references/hover/documentSymbol/workspaceSymbol/implementation/callHierarchy/diagnostics
  - `line`：行号（定位类操作必填，1-based）
  - `character`：列号（定位类操作必填，1-based）
  - `query`：workspaceSymbol 查询关键词（可选）
  - `call_hierarchy_direction`：incoming/outgoing（可选）
- 返回（JSON）：
  - `ok`：是否成功
  - `operation`：请求操作
  - `path`：文件路径
  - `results`：按 LSP 服务返回的结果列表
  - `diagnostics`：诊断摘要（errors/warnings/items），`diagnostics` 操作返回该字段

### 4.1.4 `/wunder/admin/mcp/tools`

- 方法：`POST`
- 入参（JSON）：
  - `name`：服务名称
  - `endpoint`：服务地址
- 返回（JSON）：
  - `tools`：服务端工具清单

#### `/wunder/admin/mcp/tools/call`

- 方法：`POST`
- 入参（JSON）：
  - `server`：服务名称
  - `tool`：工具名称
  - `args`：参数对象（可选）
- 返回（JSON）：
  - `result`：工具调用结果
  - `warning`：提示信息（可选）

### 4.1.4.1 `/wunder/admin/a2a`

- 方法：`GET/POST`
- `GET` 返回：
  - `services`：A2A 服务列表（name/endpoint/service_type/user_id/enabled/description/display_name/headers/auth/agent_card/allow_self/max_depth/default_method）
- `POST` 入参：
  - `services`：完整 A2A 服务列表，用于保存配置
- 说明：`service_type=internal` 表示 Wunder 内部 A2A 服务，需配置固定 `user_id` 以便挂载工具后自动填充。

### 4.1.4.2 `/wunder/admin/a2a/card`

- 方法：`POST`
- 入参（JSON）：
  - `endpoint`：A2A JSON-RPC 端点
  - `headers`：请求头对象（可选）
  - `auth`：认证字段（可选）
- 返回（JSON）：
  - `agent_card`：AgentCard 元数据

### 4.1.5 `/wunder/admin/skills`

- 方法：`GET/POST/DELETE`
- `GET` 返回：
  - `paths`：技能目录列表
  - `enabled`：已启用技能名列表
  - `skills`：技能信息（name/description/path/input_schema/enabled）
- `POST` 入参：
  - `enabled`：启用技能名列表
  - `paths`：技能目录列表（可选）
- `DELETE` 入参（Query）：
  - `name`：技能名称
- `DELETE` 返回：
  - `ok`：是否删除成功
  - `name`：已删除技能名称
  - `message`：删除说明
- 说明：仅允许删除 `EVA_SKILLS` 目录内的技能。

### 4.1.5.1 `/wunder/admin/skills/content`

- 方法：`GET`
- 入参（Query）：
  - `name`：技能名称
- 返回（JSON）：
  - `name`：技能名称
  - `path`：SKILL.md 路径
  - `content`：SKILL.md 内容

### 4.1.5.2 `/wunder/admin/skills/files`

- 方法：`GET`
- 入参（Query）：
  - `name`：技能名称
- 返回（JSON）：
  - `name`：技能名称
  - `root`：技能目录绝对路径
  - `entries`：目录结构条目（`path` 相对路径，`kind` 为 `dir/file`）

### 4.1.5.3 `/wunder/admin/skills/file`

- 方法：`GET/PUT`
- `GET` 入参（Query）：
  - `name`：技能名称
  - `path`：相对技能目录的文件路径
- `GET` 返回（JSON）：
  - `name`：技能名称
  - `path`：文件相对路径
  - `content`：文件内容
- `PUT` 入参（JSON）：
  - `name`：技能名称
  - `path`：相对技能目录的文件路径
  - `content`：文件内容
- `PUT` 返回（JSON）：
  - `ok`：是否保存成功
  - `path`：文件相对路径
  - `reloaded`：是否触发技能刷新（更新 SKILL.md 时为 true）

### 4.1.6 `/wunder/admin/llm`

- 方法：`GET/POST`
- `GET` 返回：
  - `llm.default`：默认模型配置名称
- `llm.models`：模型配置映射（model_type/provider/base_url/api_key/model/temperature/timeout_s/retry/max_rounds/max_context/max_output/support_vision/stream/stream_include_usage/tool_call_mode/history_compaction_ratio/history_compaction_reset/stop/enable/mock_if_unconfigured）
  - 说明：`retry` 同时用于请求失败重试与流式断线重连。
  - 说明：`provider` 支持 OpenAI 兼容预置（`openai_compatible/openai/openrouter/siliconflow/deepseek/moonshot/qwen/groq/mistral/together/ollama/lmstudio`），除 `openai_compatible` 外其余可省略 `base_url` 自动补齐。
  - 说明：`model_type=embedding` 表示嵌入模型，向量知识库会使用其 `/v1/embeddings` 能力。
- `POST` 入参：
  - `llm.default`：默认模型配置名称
  - `llm.models`：模型配置映射，用于保存与下发

### 4.1.6.1 `/wunder/admin/llm/context_window`

- 方法：`POST`
- 入参（JSON）：
  - `provider`：模型提供方类型（默认 openai_compatible）
  - `base_url`：模型服务地址（预置 provider 可省略）
  - `api_key`：访问密钥（可选）
  - `model`：模型名称
  - `timeout_s`：探测超时秒数（可选）
- 返回（JSON）：
  - `max_context`：最大上下文长度（可能为 null）
  - `message`：探测结果说明
  - 说明：仅支持 OpenAI 兼容 provider（见 `/wunder/admin/llm` 说明）。

### 4.1.6.2 `/wunder/admin/system`

- 方法：`GET/POST`
- `GET` 返回：
  - `server.max_active_sessions`：全局最大并发会话数
  - `server.stream_chunk_size`：流式输出分片大小（字节）
  - `server.chat_stream_channel`：聊天流式通道默认值（`ws`/`sse`）
  - `security.api_key`：API Key（未配置时为 null）
  - `security.allow_commands`：允许执行命令前缀列表
  - `security.allow_paths`：允许访问的额外目录列表
  - `security.deny_globs`：拒绝访问的路径通配规则列表
  - `security.exec_policy_mode`（allow/audit/enforce）用于高风险命令审计/拦截。
  - `sandbox.enabled`：是否启用沙盒执行（由 `sandbox.mode` 推导）
  - `sandbox.mode`：沙盒模式（local/sandbox）
  - `sandbox.endpoint`：沙盒服务地址
  - `sandbox.container_root`：容器内根目录
  - `sandbox.network`：网络模式
  - `sandbox.readonly_rootfs`：只读根文件系统开关
  - `sandbox.idle_ttl_s`：空闲回收秒数
  - `sandbox.timeout_s`：单次执行超时秒数
  - `sandbox.resources`：资源限制（cpu/memory_mb/pids）
  - `observability.log_level`：日志级别
  - `observability.monitor_event_limit`：监控事件上限
  - `observability.monitor_payload_max_chars`：监控事件内容最大字符
  - `observability.monitor_drop_event_types`：需要丢弃的事件类型
  - `cors.allow_origins`：允许来源列表
  - `cors.allow_methods`：允许方法列表
  - `cors.allow_headers`：允许请求头列表
  - `cors.allow_credentials`：是否允许携带凭证
- `POST` 入参：以上字段均可选，支持分组更新
- `POST` 返回：同 `GET`

### 4.1.6.3 `/wunder/admin/server`

- 方法：`GET/POST`
- `GET` 返回：
  - `server.max_active_sessions`：全局最大并发会话数
  - `server.sandbox_enabled`：是否启用沙盒执行（true=使用 sandbox，false=本机执行）
- `POST` 入参：
  - `max_active_sessions`：全局最大并发会话数（可选，>0）
  - `sandbox_enabled`：是否启用沙盒执行（可选）
- `POST` 返回：
  - `server.max_active_sessions`：更新后的全局最大并发会话数
  - `server.sandbox_enabled`：更新后的沙盒执行开关

### 4.1.6.4 `/wunder/admin/security`

- 方法：`GET`
- `GET` 返回：
  - `security.api_key`：当前 API Key（未配置时为 null）
- 说明：仅管理员可访问，供管理端高级设置读取默认 API Key。

### 4.1.7 `/wunder/admin/skills/upload`

- 方法：`POST`
- 入参：`multipart/form-data`
  - `file`：技能 .zip 或 .skill 压缩包
- 返回（JSON）：
  - `ok`：是否成功
  - `extracted`：解压文件数量

### 4.1.8 `/wunder/admin/monitor`

- 方法：`GET`
- 入参（Query）：
  - `active_only`：是否仅返回活动线程（默认 true）
  - `tool_hours`：统计窗口（小时，可选，用于服务状态、Sandbox 状态与工具热力图统计）
  - `start_time`：筛选开始时间戳（秒，可选，与 `end_time` 搭配时按区间统计）
  - `end_time`：筛选结束时间戳（秒，可选，与 `start_time` 搭配时按区间统计）
- 说明：当提供 `start_time`/`end_time` 时，将按区间统计并忽略 `tool_hours`；服务状态与 Sandbox 状态指标均基于统计区间。
- 返回（JSON）：
- `system`：系统资源占用（cpu_percent/memory_total/memory_used/memory_available/process_rss/process_cpu_percent/load_avg_1/load_avg_5/load_avg_15/disk_total/disk_used/disk_free/disk_percent/log_used/workspace_used/uptime_s）
  - `service`：服务状态指标（active_sessions/history_sessions/finished_sessions/error_sessions/cancelled_sessions/total_sessions/avg_token_usage/avg_elapsed_s/avg_prefill_speed_tps/avg_decode_speed_tps）
  - `sandbox`：沙盒状态（mode/network/readonly_rootfs/idle_ttl_s/timeout_s/endpoint/image/resources(cpu/memory_mb/pids)/recent_calls/recent_sessions）
  - `sessions`：活动线程列表（start_time/session_id/user_id/question/status/token_usage/elapsed_s/stage/summary
    + prefill_tokens/prefill_duration_s/prefill_speed_tps/prefill_speed_lower_bound
    + decode_tokens/decode_duration_s/decode_speed_tps）
  - `tool_stats`：工具调用统计列表（tool/calls）

### 4.1.8.1 `/wunder/admin/monitor/tool_usage`

- 方法：`GET`
- 入参（Query）：
  - `tool`：工具名称（必填）
  - `tool_hours`：统计窗口（小时，可选）
  - `start_time`：筛选开始时间戳（秒，可选，与 `end_time` 搭配时按区间统计）
  - `end_time`：筛选结束时间戳（秒，可选，与 `start_time` 搭配时按区间统计）
- 说明：当提供 `start_time`/`end_time` 时，将按区间统计并忽略 `tool_hours`。
- 返回（JSON）：
  - `tool`：工具名称
  - `tool_name`：工具真实名称（用于事件定位）
  - `sessions`：调用会话列表（session_id/user_id/question/status/stage/start_time/updated_time/elapsed_s/token_usage/tool_calls/last_time
    + prefill_tokens/prefill_duration_s/prefill_speed_tps/prefill_speed_lower_bound
    + decode_tokens/decode_duration_s/decode_speed_tps）

### 4.1.9 `/wunder/admin/monitor/{session_id}`

- 方法：`GET`
- 返回（JSON）：
  - `session`：线程详情（start_time/session_id/user_id/question/status/token_usage/elapsed_s/stage/summary
    + prefill_tokens/prefill_duration_s/prefill_speed_tps/prefill_speed_lower_bound
    + decode_tokens/decode_duration_s/decode_speed_tps）
  - `events`：事件详情列表
- 说明：
- `session` 详情新增 `log_profile`（`normal`/`debug`）与 `trace_id`，用于跨模块追踪。
- `events` 每条记录新增 `event_id`（线程内递增）。
- 每轮用户提问会额外写入 `user_input` 事件，`data.message/question` 保存原始用户消息，便于在线程详情中快速定位上下文。
- `normal` 日志画像会按 `observability.monitor_event_limit` 保留最近 N 条（<= 0 表示不截断），并按 `observability.monitor_payload_max_chars` 截断字符串字段（<= 0 表示不截断）。
- `normal` 日志画像默认跳过高频增量事件：`llm_output_delta`、`tool_output_delta`；`debug` 日志画像仅在管理员调试会话（`is_admin=true` 且 `debug_payload=true`）启用，并保留这些高频事件与完整字段。
- `llm_request` 事件仅保存 `payload_summary` 与 `message_count`，不保留完整请求体。
- `observability.monitor_drop_event_types` 主要作用于 `normal` 画像；`debug` 画像默认保留完整增量事件。
- 预填充速度基于会话第一轮 LLM 请求计算，避免多轮缓存导致速度偏高；`prefill_speed_lower_bound` 为兼容字段，当前恒为 false。


### 4.1.10 `/wunder/admin/monitor/{session_id}/cancel`

- 方法：`POST`
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
- 说明：取消会中断正在进行的 LLM/工具调用，内部轮询取消标记，通常 200ms 内生效。

### 4.1.10.1 `/wunder/admin/monitor/{session_id}/compaction`

- 方法：`POST`
- 入参（JSON）：`model_name`（可选，指定压缩摘要模型）
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
- 说明：仅会话空闲时可触发，触发后会向监控事件写入 `compaction` 记录。

### 4.1.11 `/wunder/admin/monitor/{session_id}`

- 方法：`DELETE`
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息

### 4.1.12 `/wunder/workspace`

- 说明：所有 workspace 接口支持可选 `agent_id`。若该智能体已配置 `sandbox_container_id`（1~10），则按“用户 + 容器编号”路由工作区；未传 `agent_id`、找不到智能体或历史兼容场景时，仍回退到默认用户工作区/旧路由策略。
- 方法：`GET`
- 入参（Query）：
  - `user_id`：用户唯一标识
  - `agent_id`：智能体应用 id（可选；已配置容器时按容器路由，未传或为空表示默认工作区）
  - `path`：相对路径（可选，默认根目录）
  - `refresh_tree`：是否刷新工作区树缓存（默认 false）
  - `keyword`：名称关键字过滤（可选）
  - `offset`：分页偏移量（可选）
  - `limit`：分页大小，0 表示不分页（可选）
  - `sort_by`：排序字段（name/size/updated_time）
  - `order`：排序方向（asc/desc）
- 返回（JSON）：
  - `user_id`：用户唯一标识
  - `path`：当前目录
  - `parent`：父目录（根目录为 null）
  - `entries`：目录条目（name/path/type/size/updated_time）
  - `tree_version`：工作区树版本号
  - `total`：总条目数
  - `offset`：分页偏移量
  - `limit`：分页大小

### 4.1.13 `/wunder/workspace/content`

- 方法：`GET`
- 入参（Query）：
  - `user_id`：用户唯一标识
  - `agent_id`：智能体应用 id（可选）
  - `path`：相对路径（可选，默认根目录）
  - `include_content`：是否返回内容（默认 true）
  - `max_bytes`：文件内容最大字节数（默认 512 KB）
  - `depth`：目录展开深度（默认 1）
  - `keyword`：名称关键字过滤（可选）
  - `offset`：分页偏移量（可选）
  - `limit`：分页大小（可选）
  - `sort_by`：排序字段（name/size/updated_time）
  - `order`：排序方向（asc/desc）
- 返回（JSON）：
  - `user_id`：用户唯一标识
  - `path`：当前路径
  - `type`：条目类型（file/dir）
  - `size`：文件大小（目录为 0）
  - `updated_time`：更新时间
  - `content`：文件内容（文件可选）
  - `format`：内容格式（text/dir）
  - `truncated`：是否截断
  - `entries`：目录内容条目（可选，支持 children）
  - `total`：总条目数
  - `offset`：分页偏移量
  - `limit`：分页大小

### 4.1.14 `/wunder/workspace/search`

- 方法：`GET`
- 入参（Query）：
  - `user_id`：用户唯一标识
  - `agent_id`：智能体应用 id（可选）
  - `keyword`：搜索关键字
  - `offset`：分页偏移量（可选）
  - `limit`：分页大小（可选）
  - `include_files`：是否包含文件（默认 true）
  - `include_dirs`：是否包含目录（默认 true）
- 返回（JSON）：
  - `user_id`：用户唯一标识
  - `keyword`：搜索关键字
  - `entries`：匹配条目列表（name/path/type/size/updated_time）
  - `total`：总匹配数量
  - `offset`：分页偏移量
  - `limit`：分页大小

### 4.1.15 `/wunder/workspace/upload`

- 方法：`POST`
- 入参：`multipart/form-data`
  - `user_id`：用户唯一标识
  - `agent_id`：智能体应用 id（可选）
  - `path`：相对路径（目录）
  - `files`：上传文件列表
  - `relative_paths`：文件相对路径列表（可选，保留目录结构）
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
  - `files`：已上传文件相对路径
  - `tree_version`：工作区树版本号

### 4.1.16 `/wunder/workspace/download`

- 方法：`GET`
- 入参（Query）：
  - `user_id`：用户唯一标识
  - `agent_id`：智能体应用 id（可选）
  - `path`：相对路径（文件）
- 返回：文件流

### 4.1.17 `/wunder/workspace/archive`

- 方法：`GET`
- 入参（Query）：
  - `user_id`：用户唯一标识
  - `agent_id`：智能体应用 id（可选）
  - `path`：相对路径（可选，目录/文件；留空则全量打包）
- 返回：工作区全量或指定目录的压缩包文件流

### 4.1.18 `/wunder/workspace`

- 方法：`DELETE`
- 入参（Query）：
  - `user_id`：用户唯一标识
  - `agent_id`：智能体应用 id（可选）
  - `path`：相对路径（文件或目录）
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
  - `tree_version`：工作区树版本号

### 4.1.19 `/wunder/workspace/dir`

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：用户唯一标识
  - `agent_id`：智能体应用 id（可选）
  - `path`：目录相对路径
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
  - `tree_version`：工作区树版本号
  - `files`：已创建目录路径

### 4.1.20 `/wunder/workspace/move`

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：用户唯一标识
  - `agent_id`：智能体应用 id（可选）
  - `source`：源路径
  - `destination`：目标路径
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
  - `tree_version`：工作区树版本号
  - `files`：目标路径

### 4.1.21 `/wunder/workspace/copy`

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：用户唯一标识
  - `agent_id`：智能体应用 id（可选）
  - `source`：源路径
  - `destination`：目标路径
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
  - `tree_version`：工作区树版本号
  - `files`：目标路径

### 4.1.22 `/wunder/workspace/batch`

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：用户唯一标识
  - `agent_id`：智能体应用 id（可选）
  - `action`：批量操作类型（delete/move/copy）
  - `paths`：待处理路径列表
  - `destination`：目标目录（批量移动/复制）
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
  - `tree_version`：工作区树版本号
  - `succeeded`：成功条目列表
  - `failed`：失败条目列表（path/message）

### 4.1.23 `/wunder/workspace/file`

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：用户唯一标识
  - `agent_id`：智能体应用 id（可选）
  - `path`：文件相对路径
  - `content`：文件内容
  - `create_if_missing`：文件不存在时是否创建（默认 false）
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
  - `tree_version`：工作区树版本号
  - `files`：保存的文件路径

### 4.1.24.0 `/`

- 方法：`GET`
- 说明：管理端调试前端入口（`web/index.html`），包含幻灯片（系统介绍）与 A2A 服务管理面板；`web/simple-chat` 简易聊天测试页暂时停用。
- 说明补充：管理端样式入口为 `web/app.css`，样式已拆分为 `web/styles/*.css`。

### 4.1.24.1 `/wunder/ppt`

- 方法：`GET`
- 说明：提供系统介绍 PPT 静态资源（`docs/ppt` 目录，页面拆分为 `slides/*.js`，顺序由 `slides/manifest.js` 维护），用于前端系统介绍页面嵌入或独立打开。

### 4.1.24.2 `/wunder/ppt-en`

- 方法：`GET`
- 说明：提供系统介绍 PPT 英文版静态资源（`docs/ppt-en` 目录，页面拆分为 `slides/*.js`，顺序由 `slides/manifest.js` 维护），用于前端系统介绍页面嵌入或独立打开。

### 4.1.24.3 管理端前端页面与接口

- 内部状态/线程详情：`/wunder/admin/monitor`、`/wunder/admin/monitor/tool_usage`、`/wunder/admin/monitor/{session_id}`、`/wunder/admin/monitor/{session_id}/cancel`、`/wunder/admin/monitor/{session_id}/compaction`。
- 线程管理：`/wunder/admin/users`、`/wunder/admin/users/{user_id}/sessions`、`/wunder/admin/users/{user_id}`、`/wunder/admin/users/throughput/cleanup`。
- 用户管理：`/wunder/admin/user_accounts`、`/wunder/admin/user_accounts/test/seed`、`/wunder/admin/user_accounts/{user_id}`、`/wunder/admin/user_accounts/{user_id}/password`、`/wunder/admin/user_accounts/{user_id}/tool_access`。
- 记忆管理：`/wunder/admin/memory/users`、`/wunder/admin/memory/status`、`/wunder/admin/memory/{user_id}`。
- 模型配置/系统设置：`/wunder/admin/llm`、`/wunder/admin/llm/context_window`、`/wunder/admin/system`、`/wunder/admin/server`、`/wunder/admin/security`、`/wunder/i18n`。
- 内置工具/MCP/LSP/A2A/技能/知识库：`/wunder/admin/tools`、`/wunder/admin/mcp`、`/wunder/admin/mcp/tools`、`/wunder/admin/mcp/tools/call`、`/wunder/admin/lsp`、`/wunder/admin/lsp/test`、`/wunder/admin/a2a`、`/wunder/admin/a2a/card`、`/wunder/admin/skills`、`/wunder/admin/skills/content`、`/wunder/admin/skills/files`、`/wunder/admin/skills/file`、`/wunder/admin/skills/upload`、`/wunder/admin/knowledge/*`。
- 吞吐量/性能/评估/模拟：`/wunder/admin/throughput/*`、`/wunder/admin/performance/sample`、`/wunder/admin/evaluation/*`、`/wunder/admin/sim_lab/*`。
- 调试面板接口：`/wunder`、`/wunder/system_prompt`、`/wunder/tools`、`/wunder/attachments/convert`、`/wunder/workspace/*`、`/wunder/user_tools/*`、`/wunder/cron/*`。
- 文档/幻灯片：`/wunder/ppt`、`/wunder/ppt-en`。

### 4.1.25 `/wunder/admin/tools`

- 方法：`GET/POST`
- `GET` 返回：
  - `enabled`：已启用内置工具名称列表
  - `tools`：内置工具列表（name/description/input_schema/enabled）
- `POST` 入参：
  - `enabled`：启用的内置工具名称列表

### 4.1.26 `/wunder/admin/knowledge`

- 方法：`GET/POST`
- `GET` 返回：
  - `knowledge`：知识库配置（bases 数组，元素包含 name/description/root/enabled/base_type/embedding_model/chunk_size/chunk_overlap/top_k/score_threshold）
- `POST` 入参：
  - `knowledge`：完整知识库配置，用于保存与下发
- 说明：当 root 为空时，字面知识库会自动创建 `./knowledge/<知识库名称>` 目录；向量知识库 root 自动指向 `vector_knowledge/shared/<base>` 作为逻辑标识，文档与切片元数据存储在数据库中，并要求 `embedding_model`

### 4.1.27 `/wunder/admin/knowledge/files`

- 方法：`GET`
- 入参（Query）：
  - `base`：知识库名称
- 返回（JSON）：
  - `base`：知识库名称
  - `files`：Markdown 文件相对路径列表
- 说明：仅适用于字面知识库，向量知识库请使用 `/wunder/admin/knowledge/docs` 等接口。

### 4.1.28 `/wunder/admin/knowledge/file`

- 方法：`GET/PUT/DELETE`
- `GET` 入参（Query）：
  - `base`：知识库名称
  - `path`：相对知识库根目录的文件路径
- `PUT` 入参（JSON）：
  - `base`：知识库名称
  - `path`：相对知识库根目录的文件路径
  - `content`：文件内容
- `DELETE` 入参（Query）：
  - `base`：知识库名称
  - `path`：相对知识库根目录的文件路径
- 说明：仅适用于字面知识库，向量知识库请使用 `/wunder/admin/knowledge/doc` 等接口。

### 4.1.29 `/wunder/admin/knowledge/upload`

- 方法：`POST`
- 入参（multipart/form-data）：
  - `base`：知识库名称
  - `file`：待上传文件
  - 返回（JSON）：
    - `ok`：是否成功
    - `message`：提示信息
    - `path`：转换后的 Markdown 相对路径（字面知识库）
    - `doc_id`：向量文档 id（向量知识库）
    - `doc_name`：向量文档名称（向量知识库）
    - `chunk_count`：切片数量（向量知识库）
    - `embedding_model`：嵌入模型（向量知识库）
    - `converter`：使用的转换器（doc2md/text/html/code/pdf/raw）
    - `warnings`：转换警告列表
  - 说明：该接口支持 doc2md 可解析的格式，上传后自动转换为 Markdown 保存，原始非 md 文件不会落库并会清理；向量知识库上传仅解析并切片，需通过 `/wunder/admin/knowledge/reindex` 或 `/wunder/admin/knowledge/chunk/*` 生成向量。

### 4.1.30 `/wunder/admin/knowledge/refresh`

- 方法：`POST`
- 入参（Query）：
  - `base`：知识库名称（可选，留空则刷新全部）
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
- 说明：仅适用于字面知识库，向量知识库请使用 `/wunder/admin/knowledge/reindex`。

### 4.1.30.1 `/wunder/admin/knowledge/docs`

- 方法：`GET`
- 入参（Query）：
  - `base`：知识库名称
- 返回（JSON）：
  - `base`：知识库名称
  - `docs`：向量文档列表（doc_id/name/status/chunk_count/embedding_model/updated_at）
- 说明：仅适用于向量知识库。

### 4.1.30.2 `/wunder/admin/knowledge/doc`

- 方法：`GET/DELETE`
- `GET` 入参（Query）：
  - `base`：知识库名称
  - `doc_id`：文档 id
- `GET` 返回（JSON）：
  - `base`：知识库名称
  - `doc`：文档元数据（embedding_model/chunk_size/chunk_overlap/chunk_count/status/updated_at/chunks[index/start/end/status/content]）
  - `content`：原文内容
- `DELETE` 入参（Query）：
  - `base`：知识库名称
  - `doc_id`：文档 id
- `DELETE` 返回（JSON）：
  - `ok`：是否成功
  - `deleted`：删除的向量条目数量
  - `doc_id`：文档 id
  - `doc_name`：文档名称
- 说明：仅适用于向量知识库。

### 4.1.30.3 `/wunder/admin/knowledge/chunks`

- 方法：`GET`
- 入参（Query）：
  - `base`：知识库名称
  - `doc_id`：文档 id
- 返回（JSON）：
  - `base`：知识库名称
  - `doc_id`：文档 id
  - `chunks`：切片列表（index/start/end/preview/content/status）
- 说明：仅适用于向量知识库。

### 4.1.30.4 `/wunder/admin/knowledge/chunk/update`

- 方法：`POST`
- 入参（JSON）：
  - `base`：知识库名称
  - `doc_id`：文档 id
  - `chunk_index`：切片索引
  - `content`：切片内容
- 返回（JSON）：
  - `ok`：是否成功
  - `doc`：更新后的文档元数据
- 说明：仅适用于向量知识库，更新内容后切片状态变为 `pending`。

### 4.1.30.5 `/wunder/admin/knowledge/chunk/embed`

- 方法：`POST`
- 入参（JSON）：
  - `base`：知识库名称
  - `doc_id`：文档 id
  - `chunk_index`：切片索引
- 返回（JSON）：
  - `ok`：是否成功
  - `doc`：更新后的文档元数据
- 说明：仅适用于向量知识库，执行单片嵌入并写入向量库，切片状态更新为 `embedded`。

### 4.1.30.6 `/wunder/admin/knowledge/chunk/delete`

- 方法：`POST`
- 入参（JSON）：
  - `base`：知识库名称
  - `doc_id`：文档 id
  - `chunk_index`：切片索引
- 返回（JSON）：
  - `ok`：是否成功
  - `doc`：更新后的文档元数据
- 说明：仅适用于向量知识库，删除切片向量并标记为 `deleted`。

### 4.1.30.7 `/wunder/admin/knowledge/test`

- 方法：`POST`
- 入参（JSON）：
  - `base`：知识库名称
  - `query`：测试问题
  - `top_k`：召回数量（可选，默认使用知识库配置）
- 返回（JSON）：
  - `base`：知识库名称
  - `query`：测试问题
  - 向量知识库：
    - `embedding_model`：嵌入模型
    - `top_k`：召回数量
    - `hits`：召回结果列表
      - `doc_id`：文档 id
      - `document`：文档名称
      - `chunk_index`：切片索引
      - `start`：切片起点
      - `end`：切片终点
      - `content`：切片内容
      - `score`：相似度分数
  - 字面知识库：
    - `text`：模型原始输出
    - `hits`：命中文档列表
      - `doc_id`：文档编码
      - `document`：文档名称
      - `content`：文档内容
      - `score`：相关度分数（可选）
      - `section_path`：章节路径
      - `reason`：命中原因（可选）
- 说明：字面知识库会调用大模型生成原始输出，并附带命中文档内容；向量知识库保持召回结果。

### 4.1.30.8 `/wunder/admin/knowledge/reindex`

- 方法：`POST`
- 入参（JSON）：
  - `base`：知识库名称
  - `doc_id`：文档 id（可选，留空则重建全部）
- 返回（JSON）：
  - `ok`：是否成功
  - `reindexed`：已重建的 doc_id 列表
  - `failed`：失败项列表（doc_id/error）
- 说明：仅适用于向量知识库，执行重建嵌入。

### 4.1.31 `/wunder/admin/users`

- 方法：`GET`
- 返回（JSON）：
  - `users`：用户统计列表
    - `user_id`：用户标识
    - `active_sessions`：活动线程数
    - `history_sessions`：历史线程数
    - `total_sessions`：会话总数
    - `chat_records`：历史对话记录条数
    - `tool_calls`：工具调用次数
    - `token_usage`：累计占用的 Token 总量

### 4.1.32 `/wunder/admin/users/{user_id}/sessions`

- 方法：`GET`
- 入参（Query）：
  - `active_only`：是否仅返回活动线程（默认 false）
- 返回（JSON）：
  - `user_id`：用户标识
  - `sessions`：会话列表（字段同 `/wunder/admin/monitor` 的 sessions）

### 4.1.33 `/wunder/admin/users/{user_id}`

- 方法：`DELETE`
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
  - `cancelled_sessions`：已终止的活动线程数量
  - `deleted_sessions`：已清除的会话数量
  - `deleted_chat_records`：已删除的对话记录数
  - `deleted_tool_records`：已删除的工具日志数
  - `workspace_deleted`：工作区是否删除
  - `legacy_history_deleted`：旧版历史目录是否删除

### 4.1.34 `/wunder/admin/users/throughput/cleanup`

- 方法：`POST`
- 入参（JSON，可选）：
  - `prefix`：压测用户前缀，默认 `throughput_user`
- 返回（JSON）：
  - `ok`：是否成功
  - `prefix`：匹配前缀
  - `users`：清理的用户数量
  - `cancelled_sessions`：终止的活动线程数量
  - `deleted_sessions`：清除的会话数量
  - `deleted_storage`：持久化存储中删除的会话数量
  - `deleted_chat_records`：删除的对话记录数
  - `deleted_tool_records`：删除的工具日志数
  - `workspace_deleted`：删除的工作区数量

### 4.1.35 `/wunder/admin/memory/users`

- 方法：`GET`
- 返回（JSON）：
  - `users`：长期记忆用户列表
    - `user_id`：用户标识
    - `enabled`：是否启用长期记忆
    - `record_count`：记忆记录数量
    - `last_updated_time`：最近更新时间（ISO）
    - `last_updated_time_ts`：最近更新时间戳（秒）

### 4.1.36 `/wunder/admin/memory/status`

- 方法：`GET`
- 返回（JSON）：
  - `active`：活动队列任务列表（包含正在处理与排队中）
    - `task_id`：任务标识
    - `user_id`：用户标识
    - `session_id`：会话标识
    - `status`：任务状态（正在处理/排队中）
    - `queued_time`：排队时间（ISO）
    - `queued_time_ts`：排队时间戳（秒）
    - `started_time`：开始时间（ISO）
    - `started_time_ts`：开始时间戳（秒）
    - `finished_time`：完成时间（ISO）
    - `finished_time_ts`：完成时间戳（秒）
    - `elapsed_s`：耗时（秒）
  - `history`：历史队列任务列表（字段同上，状态为已完成/失败）

### 4.1.37 `/wunder/admin/memory/status/{task_id}`

- 方法：`GET`
- 返回（JSON）：
  - `task_id`：任务标识
  - `user_id`：用户标识
  - `session_id`：会话标识
  - `status`：任务状态
  - `queued_time`：排队时间（ISO）
  - `queued_time_ts`：排队时间戳（秒）
  - `started_time`：开始时间（ISO）
  - `started_time_ts`：开始时间戳（秒）
  - `finished_time`：完成时间（ISO）
  - `finished_time_ts`：完成时间戳（秒）
  - `elapsed_s`：耗时（秒）
  - `request`：记忆总结请求载荷（messages/tool_names/model_name/config_overrides 等）
  - `result`：记忆总结结果（纯文本段落）
  - `error`：失败原因（无则为空）

### 4.1.38 `/wunder/admin/memory/{user_id}`

- 方法：`GET`
- 返回（JSON）：
  - `user_id`：用户标识
  - `enabled`：是否启用长期记忆
  - `records`：记忆记录列表
    - `session_id`：会话标识
    - `summary`：记忆内容（纯文本段落）
    - `created_time`：创建时间（ISO）
    - `updated_time`：更新时间（ISO）
    - `created_time_ts`：创建时间戳（秒）
    - `updated_time_ts`：更新时间戳（秒）

### 4.1.39 `/wunder/admin/memory/{user_id}/{session_id}`

- 方法：`PUT`
- 入参（JSON）：
  - `summary`：记忆内容（纯文本段落）
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息

### 4.1.40 `/wunder/admin/memory/{user_id}/enabled`

- 方法：`POST`
- 入参（JSON）：
  - `enabled`：是否启用长期记忆
- 返回（JSON）：
  - `user_id`：用户标识
  - `enabled`：是否启用长期记忆

### 4.1.41 `/wunder/admin/memory/{user_id}/{session_id}`

- 方法：`DELETE`
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
  - `deleted`：删除条数

### 4.1.42 `/wunder/admin/memory/{user_id}`

- 方法：`DELETE`
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
  - `deleted`：删除条数

### 4.1.43 `/wunder/admin/throughput/start`

- 方法：`POST`
- 入参（JSON）：
  - `concurrency_list`：并发列表（必填，数组；每个值 >0 且 <=500）
  - `user_id_prefix`：用户前缀（可选，默认 `throughput_user`）
  - `model_name`：模型配置名称（可选，不传使用默认模型）
  - `max_tokens`：单次最大输出 Token（可选，<=0 或不传表示使用模型默认）
  - `request_timeout_s`：单次请求超时（可选，<=0 表示不启用）
- 说明：
  - 服务端按 `concurrency_list` 顺序逐档压测，每个档位只发送一轮并发请求。
  - 压测问题使用内置题库（50 条），每次请求随机抽取。
  - 并发上限仍受 `server.max_active_sessions` 影响，超过上限会在服务端排队。
- 返回（JSON）：`ThroughputSnapshot`

### 4.1.44 `/wunder/admin/throughput/stop`

- 方法：`POST`
- 返回（JSON）：`ThroughputSnapshot`
- 说明：仅停止新请求，已在执行中的请求会继续完成；状态会先变为 `stopping`，全部结束后变为 `stopped`。

### 4.1.45 `/wunder/admin/throughput/status`

- 方法：`GET`
- 返回（JSON）：
  - `active`：当前压测任务快照（`ThroughputSnapshot`，无则为 null）
  - `history`：历史压测快照数组（最多保留 50 条）

### 4.1.46 `/wunder/admin/throughput/report`

- 方法：`GET`
- 入参（Query）：
  - `run_id`：压测任务 ID（可选；不传则优先返回运行中任务，否则返回最近一次结果）
- 返回（JSON）：`ThroughputReport`（包含汇总快照与采样序列）
- 说明：报告会持久化到 `data/throughput`，便于导出与回溯。

#### ThroughputSnapshot

- `run`：任务信息
  - `id`：任务 ID
  - `status`：`running/stopping/finished/stopped`
  - `max_concurrency`：最大并发（为 `concurrency_list` 的最大值）
  - `concurrency_list`：并发列表
  - `question_set`：题库标识（内置为 `builtin`）
  - `question_count`：题库问题数量
  - `user_id_prefix`：用户前缀
  - `stream`：是否流式（固定 true）
  - `model_name`：模型配置（默认 null，表示使用默认模型）
  - `request_timeout_s`：单次请求超时（秒）
  - `max_tokens`：单次最大输出 Token（可选）
  - `started_at`：开始时间（RFC3339）
  - `finished_at`：结束时间（RFC3339，可选）
  - `elapsed_s`：已运行时长（秒）
- `metrics`：汇总指标
  - `total_requests`：请求总数
  - `success_requests`：成功数
  - `error_requests`：失败数
  - `rps`：每秒请求数（四舍五入到两位小数）
  - `avg_latency_ms`：平均耗时（毫秒）
  - `first_token_latency_ms`：首包延迟（毫秒）
  - `min_latency_ms`：最小耗时（毫秒）
  - `max_latency_ms`：最大耗时（毫秒）
  - `p50_latency_ms`：P50 耗时（毫秒，基于桶估算）
  - `p90_latency_ms`：P90 耗时（毫秒，基于桶估算）
  - `p99_latency_ms`：P99 耗时（毫秒，基于桶估算）
  - `input_tokens/output_tokens/total_tokens`：累计 token 统计
  - `avg_total_tokens`：平均 token（按成功请求统计）
  - `latency_buckets`：延迟桶统计（`le_ms` 为上界，null 表示超过最大上界）
- `errors`：最近错误列表（最多 20 条）

#### ThroughputReport

- `summary`：压测快照（`ThroughputSnapshot`）
- `samples`：采样序列（`ThroughputSample`）

#### ThroughputSample

- `timestamp`：采样时间（RFC3339）
- `concurrency`：当前并发档位
- `elapsed_s`：该档位耗时（秒）
- `total_requests/success_requests/error_requests`：该档位请求指标
- `rps`：该档位吞吐
- `avg_latency_ms`：平均耗时（毫秒）
- `p50_latency_ms/p90_latency_ms/p99_latency_ms`：延迟分位（毫秒）
- `total_prefill_speed_tps`：总预填充速度（token/s）
- `single_prefill_speed_tps`：单预填充速度（token/s）
- `total_decode_speed_tps`：总解码速度（token/s）
- `single_decode_speed_tps`：单解码速度（token/s）
- `input_tokens/output_tokens/total_tokens`：该档位 token 统计
- `avg_total_tokens`：平均 token（按成功请求统计）

### 4.1.47 `/wunder/admin/performance/sample`

- 方法：`POST`
- 入参（JSON）：
  - `concurrency`：并发数（>0 且 <= `server.max_active_sessions`）
  - `command`：执行命令内容（可选，默认 `echo wunder_perf`）
- 返回（JSON）：
  - `concurrency`：并发数
  - `metrics`：指标数组
    - `key`：指标标识（`prompt_build`/`file_ops`/`command_exec`/`tool_access`/`log_write`）
    - `avg_ms`：平均耗时（毫秒，可能为 null）
    - `ok`：是否全部成功
    - `error`：错误信息（可选）
- 说明：
  - `prompt_build`：系统提示词构建耗时。
  - `file_ops`：列出文件/写入/读取/搜索/替换文本组合耗时。
  - `command_exec`：内置工具“执行命令”的耗时。
  - `tool_access`：用户工具绑定与权限解析的耗时。
  - `log_write`：写入工具日志耗时。
  - 每个并发点会执行两轮采样，返回两轮平均耗时。
  - 用于不同并发下的性能采样，不涉及模型调用。

### 4.1.48 `/wunder/admin/evaluation/start`

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：用户唯一标识
  - `model_name`：模型配置名称（可选）
  - `language`：语言（可选，默认使用系统语言）
  - `case_set`：用例集名称（可选，默认 `default`）
  - `dimensions`：维度列表（可选，`tool/logic/common/complex`，为空表示全部）
  - `weights`：维度权重对象（可选，`tool/logic/common/complex`，总和会归一到 100，默认 35/25/20/20）
  - `tool_names`：启用的工具名称列表（可选，未传使用管理员默认启用工具）
  - `config_overrides`：临时覆盖配置对象（可选）
- 返回（JSON）：
  - `run_id`：评估任务 ID
  - `status`：任务状态（`running`）
  - `case_count`：用例数量

### 4.1.49 `/wunder/admin/evaluation/{run_id}/cancel`

- 方法：`POST`
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：失败原因（可选）

### 4.1.50 `/wunder/admin/evaluation/runs`

- 方法：`GET`
- 入参（Query）：
  - `user_id`：用户唯一标识（可选）
  - `status`：状态筛选（可选）
  - `model_name`：模型配置名称（可选）
  - `since_time`：开始时间下限（秒级时间戳，可选）
  - `until_time`：开始时间上限（秒级时间戳，可选）
  - `limit`：返回条数上限（可选）
- 返回（JSON）：
  - `runs`：评估任务列表（`EvaluationRun`）

### 4.1.51 `/wunder/admin/evaluation/{run_id}`

- 方法：`GET/DELETE`
- `GET` 返回（JSON）：
  - `run`：评估任务（`EvaluationRun`）
  - `items`：评估明细列表（`EvaluationItem`）
- `DELETE` 返回（JSON）：
  - `ok`：是否删除成功
  - `run_id`：评估任务 ID
  - `deleted`：删除条数（包含 run 与 items）
  - `message`：提示信息

### 4.1.52 `/wunder/admin/evaluation/cases`

- 方法：`GET`
- 返回（JSON）：
  - `case_sets`：用例集摘要列表
    - `case_set`：用例集名称
    - `language`：语言
    - `version`：版本号
      - `case_count`：用例数量
      - `dimensions`：维度分布统计（维度 -> 数量）
- 说明：评估用例文件默认读取 `config/evaluation/cases`。

### 4.1.53 `/wunder/admin/evaluation/stream/{run_id}`

- 方法：`GET`（SSE）
- 事件：
  - `eval_started`：评估开始（`run_id`、`case_count`）
  - `eval_item`：用例状态更新（`EvaluationItem`，含 `active` 与最终结果）
  - `eval_progress`：进度更新（`completed/total/passed/failed/skipped/errors`）
  - `eval_finished`：评估结束（`EvaluationRun`）
  - `eval_log`：日志提示（取消请求等）

#### EvaluationRun

- `run_id`：评估任务 ID
- `user_id`：用户标识
- `status`：`running/finished/failed/cancelled`
- `model_name`：模型配置名称
- `language`：语言
- `case_set`：用例集
- `dimensions`：评估维度列表
- `weights`：维度权重
- `total_score`：总分（0~100）
- `dimension_scores`：维度评分映射（0~100）
- `case_count/passed_count/failed_count/skipped_count/error_count`：用例统计
- `started_time/finished_time/elapsed_s`：时间信息（秒级时间戳/耗时）
- `tool_names`：请求时传入的工具清单
- `tool_snapshot`：评估时实际可用工具快照
- `case_ids`：本次评估使用的用例 ID 列表
- `error`：错误信息（可选）
- `config_overrides`：评估时使用的配置覆盖（可选）

#### EvaluationItem

- `run_id`：评估任务 ID
- `case_id`：用例 ID
- `dimension`：维度（`tool/logic/common/complex`）
- `status`：`active/passed/failed/skipped/error/cancelled`
- `score/max_score/weight`：得分、满分（按维度权重分摊的分值）与权重
- `prompt`：实际评估提示词
- `checker`：判定器配置
- `final_answer`：模型最终回复
- `tool_calls`：工具调用记录
- `checker_detail`：判定详情
- `skip_reason`：跳过原因（可选）
- `started_time/finished_time/elapsed_s`：时间信息（秒级时间戳/耗时）
- `error`：错误信息（可选）
- `session_id`：评估用会话标识

### 4.1.54 `/wunder/auth/*`

- `POST /wunder/auth/register`
  - 入参（JSON）：`username`、`email`（可选）、`password`、`unit_id`（可选）
  - 返回（JSON）：`data.access_token`、`data.user`（UserProfile）
- `POST /wunder/auth/login`
  - 入参（JSON）：`username`、`password`
  - 返回：同注册
- `POST /wunder/auth/demo`
  - 入参（JSON）：`demo_id`（可选）
  - 返回：同注册
- `GET /wunder/auth/org_units`
  - 入参：无
  - 返回（JSON）：`data.items`（单位列表）、`data.tree`（单位树）
- `GET /wunder/auth/me`
  - 鉴权：Bearer Token
  - 返回（JSON）：`data`（UserProfile）
- `PATCH /wunder/auth/me`
  - 鉴权：Bearer Token
  - 入参（JSON）：`username`（可选）、`email`（可选）、`unit_id`（可选）
  - 返回（JSON）：`data`（UserProfile）
- 错误返回：统一结构见“4.0.1 统一错误响应（HTTP）”；兼容字段仍保留 `detail.message`。

#### UserProfile

- `id`：用户 ID
- `username`：用户名
- `email`：邮箱（可选）
- `roles`：角色列表
- `status`：账号状态（active/disabled）
- `access_level`：访问级别（保留字段，当前统一为 A）
- `unit_id`：所属单位 ID（可选）
- `unit`：所属单位信息（可选，`id/name/path/path_name/level`）
- `daily_quota`：每日额度
- `daily_quota_used`：今日已用额度
- `daily_quota_date`：额度日期（可选）
- `is_demo`：是否演示账号
- `created_at`/`updated_at`：时间戳（秒）
- `last_login_at`：最近登录时间（秒，可选）


### 4.1.54.1 `/wunder/channel/{provider}/webhook`

- 方法：`POST`
- 鉴权：可选（按渠道账号配置 inbound_token 校验）
- Path 参数：`provider` 渠道标识（如 telegram/wecom/web）
- Query：
  - `account_id`：可选，覆盖 ChannelMessage.account_id
- Header：
  - `x-channel-account`：可选，覆盖 account_id
  - `x-channel-token` 或 `Authorization: Bearer <token>`：用于 inbound_token 校验
- 入参（JSON）：
  - 单条 ChannelMessage
  - ChannelMessage 数组
  - 包装结构 `{ messages: [ChannelMessage, ...] }`
- 返回（JSON）：`data.accepted`、`data.session_ids`、`data.outbox_ids`、`data.errors`
- 说明：raw_payload 与标准化消息会落库到 `channel_messages`。

#### ChannelMessage

- `channel`：渠道名称（可空；为空时使用 URL 中 provider）
- `account_id`：渠道账号标识
- `peer`：`{ kind, id, name? }`
- `thread`：`{ id, topic? }`（可选）
- `message_id`：渠道消息 id（可选）
- `sender`：`{ id, name? }`（可选）
- `type`：`text|image|audio|video|location|file|mixed`
- `text`：文本内容（可选）
- `attachments`：`[{ kind,url,mime?,size?,name? }]`
- `location`：`{ lat,lng,address? }`
- `ts`：时间戳（可选）
- `meta`：扩展字段（可选）

### 4.1.54.2 `/wunder/channel/whatsapp/webhook`（WhatsApp Cloud）

- 方法：`GET/POST`
- `GET`（订阅校验）：
  - Query：`hub.mode=subscribe`、`hub.verify_token`、`hub.challenge`
  - 返回：`hub.challenge`
  - 校验规则：在 `channels.accounts` 中查找 `whatsapp_cloud.verify_token` 匹配成功即通过。
- `POST`（消息回调）：
  - 入参：WhatsApp Cloud Webhook 原始 payload（`object=whatsapp_business_account`）
  - Header：`X-Hub-Signature-256`（可选；当 `whatsapp_cloud.app_secret` 配置存在时强制校验）
  - 账号识别：默认使用 `metadata.phone_number_id` 作为 `account_id`，可通过 Query `account_id` 或 `x-channel-account` 覆盖。
  - 出站投递：当 `whatsapp_cloud.access_token` 可用时直接走 Cloud API；否则回退到 `outbound_url`。
- 配置（ChannelAccount.config）：
  - `whatsapp_cloud.phone_number_id`
  - `whatsapp_cloud.access_token`
  - `whatsapp_cloud.verify_token`
  - `whatsapp_cloud.app_secret`
  - `whatsapp_cloud.api_version`（默认 `v20.0`）
- 说明：
  - 当前入站媒体消息会保留 `meta.media` 与提示文本，附件下载需后续扩展（Cloud 媒体 URL 需要鉴权）。
  - 建议先通过管理员接口创建 `whatsapp` 渠道账户，再完成 Meta Webhook 订阅。

### 4.1.54.3 `/wunder/channel/feishu/webhook`（飞书/多维表）

- 方法：`POST`
- 鉴权：
  - 若账号配置了 `feishu.verification_token`，URL 验证阶段会校验 payload `token`
  - 若账号配置了 `feishu.encrypt_key` 且请求头携带 `x-lark-signature`，会执行签名校验
- Query：
  - `account_id`：可选，覆盖渠道账号（默认尝试从 `header.app_id` 推断）
- Header：
  - `x-channel-account`：可选，覆盖 account_id
  - `x-lark-request-timestamp` / `x-lark-request-nonce` / `x-lark-signature`：签名校验用
- URL 验证：payload 包含 `challenge` 时返回 `{ "challenge": "..." }`
- 消息回调：
  - 当前支持解析 `event.message`（text/image/file/audio/media）
  - 生成标准 `ChannelMessage` 进入主链路：`user_id -> agent_id -> session_id -> agent_loop`
- 加密回调：
  - 当 payload 包含 `encrypt` 且账号配置了 `feishu.encrypt_key` 时，服务端会先解密再按事件处理
  - 解密流程遵循飞书事件订阅 AES-CBC + PKCS7 规范（IV 前置、网络字节序长度字段）
- 出站投递：
  - 当 `ChannelAccount.config.feishu.app_id/app_secret` 可用时，使用飞书 OpenAPI 直接发送文本
  - 默认 `receive_id_type=chat_id`（可通过 `feishu.receive_id_type` 覆盖）
- 长连接模式：
  - 当渠道账号 `status=active` 且配置了 `feishu.app_id/app_secret` 时，系统会自动建立飞书长连接。
- `account_id`：渠道账号标识，仅用于 wunder 内部区分账号，不是飞书消息路由 ID（通常可填 `cli_xxx` 便于识别）。
- `peer_id`：用户侧绑定的对端 ID，不能填 app_id/cli；当 `receive_id_type=chat_id` 时应填写 `chat_id`（如 `oc_xxx`）。
- `peer_kind`：飞书建议仅使用 `user`（私聊）或 `group`（群聊）。
- 可通过管理端模板配置 `feishu.long_connection_enabled=false` 临时关闭长连接。
  - worker 会定期请求 `POST https://{feishu.domain}/callback/ws/endpoint` 获取 WS 地址并保持连接。
  - 收到消息后统一走 ChannelHub 主链路并触发智能体回复。


### 4.1.54.4 `/wunder/channel/qqbot/webhook`（QQ Bot）

- 方法：`POST`
- 鉴权：可选（由 `inbound_token` 统一控制）
- Query：
  - `account_id`：可选，覆盖渠道账号（默认尝试从 payload `app_id/appid` 推断）
- Header：
  - `x-channel-account`：可选，覆盖 account_id
- 入参（JSON）：
  - 支持直接消息对象，或 `{ d: ... }` 包装体
  - 识别字段：`content`、`id/msg_id`、`author.member_openid/author.id`、`group_openid`
- 路由规则：
  - 有 `group_openid` -> `peer.kind=group`
  - 否则 -> `peer.kind=user`
- 出站投递：
  - 当 `ChannelAccount.config.qqbot.app_id/client_secret` 可用时，调用 QQ Bot API 直连发送
  - `peer.kind=user` -> `/v2/users/{openid}/messages`
  - `peer.kind=group` -> `/v2/groups/{group_openid}/messages`
  - `peer.kind=channel` -> `/channels/{channel_id}/messages`

### 4.1.55 `/wunder/chat/*`

- `GET /wunder/chat/transport`：获取当前聊天流式通道策略
  - 返回：`data.chat_stream_channel`（`ws`/`sse`）

- `POST /wunder/chat/sessions`：创建会话
  - 入参（JSON）：`title`（可选）、`agent_id`（可选）
- 返回：`data`（id/title/created_at/updated_at/last_message_at/agent_id/tool_overrides/parent_session_id/parent_message_id/spawn_label/spawned_by/is_main）
- `GET /wunder/chat/sessions`：会话列表
  - Query：`page`/`page_size` 或 `offset`/`limit`，可选 `agent_id`（空值表示通用聊天，省略表示不过滤），可选 `parent_session_id`（或 `parent_id`/`parentId`/`parentSessionId`）
- 返回：`data.total`、`data.items`（每项含 is_main 标记主线程）
- `GET /wunder/chat/sessions/{session_id}`：会话详情
  - Query：`limit`（消息条数，可选）
  - 返回：`data`（会话信息含 parent_session_id/parent_message_id/spawn_label/spawned_by + messages；进行中的会话会追加 stream_incomplete=true 的助手占位）
- `GET /wunder/chat/sessions/{session_id}/events`：会话事件（工作流还原）
  - 返回：`data.id`、`data.rounds`（user_round/events；事件内包含 `user_round`/`model_round`）、`data.running`、`data.last_event_id`
- `DELETE /wunder/chat/sessions/{session_id}`：删除会话
  - 会同时删除该会话关联的定时任务
  - 返回：`data.id`
- `POST /wunder/chat/sessions/{session_id}/messages`：发送消息（支持 SSE）
  - 入参（JSON）：`content`、`stream`（默认 true）、`attachments`（可选）
  - 会话系统提示词首次构建后固定用于历史还原，工具可用性仍以当前配置与选择为准
  - 注册用户每日请求额度超额时返回 429（`detail.code=USER_QUOTA_EXCEEDED`）
- `GET /wunder/chat/sessions/{session_id}/resume`：恢复流式（SSE）
  - Query：`after_event_id`（可选，传入则回放并持续推送后续事件；不传则仅推送新产生的事件）
- `POST /wunder/chat/sessions/{session_id}/cancel`：取消会话
  - 返回：`data.cancelled`
- `POST /wunder/chat/sessions/{session_id}/compaction`：主动触发会话上下文压缩
  - 入参（JSON）：`model_name`（可选，指定压缩模型）
  - 限制：会话处于 `running`/`cancelling`/`waiting` 时返回 409
  - 返回：`data.ok`、`data.message`
- `POST /wunder/chat/sessions/{session_id}/tools`：设置会话工具覆盖
  - 入参（JSON）：`tool_overrides`（字符串数组，空数组表示恢复默认；传入 `__no_tools__` 表示禁用全部工具）
  - 返回：`data.id`、`data.tool_overrides`
- `POST /wunder/chat/system-prompt`：系统提示词预览
  - 入参（JSON）：`agent_id`（可选）、`tool_overrides`（可选）
  - 返回：`data.prompt`
- `POST /wunder/chat/sessions/{session_id}/system-prompt`：会话系统提示词预览（校验会话归属）
  - 会话已保存提示词时直接返回该版本，不受工具勾选或工作区变化影响
  - 未保存时才按 `agent_id/tool_overrides` 构建当前提示词预览
  - 入参（JSON）：`agent_id`（可选）、`tool_overrides`（可选）
  - 返回：`data.prompt`
- `POST /wunder/chat/attachments/convert`：附件转换
  - 入参：`multipart/form-data`，`file`（支持多个同名字段）
  - 返回：`data`（单文件为转换结果；多文件返回 `items` 列表，元素含 `name`/`content`/`converter`/`warnings`）

### 4.1.56 `/wunder/agents`

- `GET /wunder/agents`：智能体列表
  - 返回：`data.total`、`data.items`（id/name/description/system_prompt/tool_names/access_level/is_shared/status/icon/sandbox_container_id/created_at/updated_at）
- `GET /wunder/agents/shared`：共享智能体列表
  - 返回：`data.total`、`data.items`（同上）
- `GET /wunder/agents/running`：当前运行中的智能体会话锁 + 问询面板待选择状态
  - 返回：`data.total`、`data.items`（agent_id/session_id/updated_at/expires_at/state/pending_question/is_default）
  - `is_default`：表示通用聊天（无 agent_id 的默认入口会话）
  - `state`：`running` | `waiting`，`pending_question` 表示存在待选择问询面板
- `POST /wunder/agents`：创建智能体
  - 入参（JSON）：`name`（必填）、`description`（可选）、`system_prompt`（可选）、`tool_names`（可选）、`is_shared`（可选）、`status`（可选）、`icon`（可选）、`sandbox_container_id`（可选，1~10，默认 1）
  - 返回：`data`（同智能体详情）
- `GET /wunder/agents/{agent_id}`：智能体详情
  - 返回：`data`（同智能体详情）
- `PUT /wunder/agents/{agent_id}`：更新智能体
  - 入参（JSON）：`name`/`description`/`system_prompt`/`tool_names`/`is_shared`/`status`/`icon`/`sandbox_container_id`（可选）
  - 返回：`data`（同智能体详情）
- `DELETE /wunder/agents/{agent_id}`：删除智能体
  - 返回：`data.id`
- `GET /wunder/agents/{agent_id}/default-session`：获取智能体主线程会话
  - 返回：`data.agent_id`、`data.session_id`
- `POST /wunder/agents/{agent_id}/default-session`：设置智能体主线程会话
  - 入参（JSON）：`session_id`
  - 返回：`data.agent_id`、`data.session_id`、`data.thread_id`、`data.status`、`data.updated_at`
  - 说明：默认入口使用 `agent_id=__default__`
- 说明：
  - 智能体提示词会追加到基础系统提示词末尾。
  - `tool_names` 会按用户工具白名单过滤。
  - 共享智能体对所有用户可见，管理员可通过单用户权限覆盖进一步调整。
  - 首次读取智能体列表会按 `config/wunder.yaml` 的 `user_agents.presets` 自动补齐默认智能体，可通过配置调整数量与内容。
  - `sandbox_container_id` 取值范围 1~10，默认 1；同一用户下相同容器编号的智能体共享同一文件工作区。

### 4.1.57 `/wunder/admin/user_accounts/*`

- `GET /wunder/admin/user_accounts`
  - Query：`keyword`、`offset`、`limit`
  - 返回：`data.total`、`data.items`（UserProfile + `daily_quota`/`daily_quota_used`/`daily_quota_remaining`/`daily_quota_date`）
- `POST /wunder/admin/user_accounts`
  - 入参（JSON）：`username`、`email`（可选）、`password`、`unit_id`（可选）、`roles`（可选）、`status`（可选）、`is_demo`（可选）
  - 返回：`data`（UserProfile）
- `POST /wunder/admin/user_accounts/test/seed`
  - 入参（JSON）：`per_unit`（每单位新增数量，1~200）
  - 返回：`data.created`、`data.unit_count`、`data.per_unit`、`data.password`
- `PATCH /wunder/admin/user_accounts/{user_id}`
  - 入参（JSON）：`email`（可选）、`status`（可选）、`unit_id`（可选）、`roles`（可选）、`daily_quota`（可选）
  - 返回：`data`（UserProfile）
- `DELETE /wunder/admin/user_accounts/{user_id}`
  - 返回：`data.ok`、`data.id`
- `POST /wunder/admin/user_accounts/{user_id}/password`
  - 入参（JSON）：`password`
  - 返回：`data.ok`
- `GET /wunder/admin/user_accounts/{user_id}/tool_access`
  - 返回：`data.allowed_tools`（null 表示使用默认策略）
- `PUT /wunder/admin/user_accounts/{user_id}/tool_access`
  - 入参（JSON）：`allowed_tools`（null 或字符串数组）
  - 返回：`data.allowed_tools`
- `GET /wunder/admin/user_accounts/{user_id}/agent_access`
  - 返回：`data.allowed_agent_ids`（null 表示使用默认策略）、`data.blocked_agent_ids`
- `PUT /wunder/admin/user_accounts/{user_id}/agent_access`
  - 入参（JSON）：`allowed_agent_ids`（null 或字符串数组）、`blocked_agent_ids`（可选字符串数组）
  - 返回：`data.allowed_agent_ids`、`data.blocked_agent_ids`
- 说明：管理员或单位负责人可访问；单位负责人仅能管理本单位及下级单位用户。

### 4.1.58 `/wunder/admin/org_units/*`

- `GET /wunder/admin/org_units`
  - 返回：`data.items`（OrgUnit 列表）、`data.tree`（树状结构）
- `POST /wunder/admin/org_units`
  - 入参（JSON）：`name`（必填）、`parent_id`（可选）、`sort_order`（可选）、`leader_ids`（可选）
  - 返回：`data`（OrgUnit）
- `PATCH /wunder/admin/org_units/{unit_id}`
  - 入参（JSON）：`name`（可选）、`parent_id`（可选）、`sort_order`（可选）、`leader_ids`（可选）
  - 返回：`data`（OrgUnit）
- `DELETE /wunder/admin/org_units/{unit_id}`
  - 返回：`data.unit_id`
- 说明：管理员或单位负责人可访问；单位负责人仅能管理本单位及下级单位。

#### OrgUnit

- `unit_id`：单位 ID
- `parent_id`：上级单位 ID（可选）
- `name`：名称
- `level`：层级（1~4）
- `path`：路径 ID 串
- `path_name`：路径名称串
- `sort_order`：同级排序
- `leader_ids`：负责人用户 ID 列表
- `created_at`/`updated_at`：时间戳（秒）

### 4.1.58.1 `/wunder/admin/external_links/*`

- `GET /wunder/admin/external_links`
  - Returns: `data.items` (ExternalLink list, includes enabled and disabled items)
- `POST /wunder/admin/external_links`
  - JSON body:
    - `link_id`: optional; update when provided, auto-generate `ext_{uuid}` when omitted
    - `title`: required app name
    - `description`: optional description
    - `url`: required, only `http`/`https` allowed
    - `icon`: optional, defaults to `fa-globe`
    - `allowed_levels`: optional `1~4` integer array; empty array means visible to all levels
    - `sort_order`: optional, defaults to `0`
    - `enabled`: optional, defaults to `true`
  - Returns: `data` (ExternalLink)
- `DELETE /wunder/admin/external_links/{link_id}`
  - Returns: `data.link_id`
- Notes: admin-only endpoints used by the admin "External Link Management" panel.

#### ExternalLink

- `link_id`: link identifier
- `title`: app name
- `description`: description
- `url`: target URL
- `icon`: Font Awesome icon config; supports plain class string (`fa-globe`) or JSON string (`{"name":"fa-globe","color":"#2563eb"}`)
- `allowed_levels`: allowed org levels (empty means all)
- `sort_order`: sort value (ascending)
- `enabled`: enable status
- `created_at`/`updated_at`: unix timestamp (seconds)

### 4.1.58.2 `/wunder/external_links`

- `GET /wunder/external_links`
  - Auth: user token (`Authorization: Bearer <user_token>`)
  - Query: `link_id` (optional, exact filter)
  - Returns: `data.items` (ExternalLink list filtered by current user's org level), `data.user_level`
- Filter rules:
  - `enabled=false` items are not returned
  - Empty `allowed_levels` means visible to all levels
  - Non-empty `allowed_levels` means only matching levels can see the item

### 4.1.59 `/wunder/admin/channels/*`

- `GET /wunder/admin/channels/accounts`
  - Query：`channel`、`status`
  - 返回：`data.items`（channel/account_id/config/status/created_at/updated_at/runtime）
  - `runtime.feishu_long_connection`：飞书账号运行态（`running/missing_credentials/disabled/account_inactive/not_configured`）与 `binding_count`
- `POST /wunder/admin/channels/accounts`
  - 入参：`channel`、`account_id`、`config`、`status`（可选）
  - 返回：账号记录（含 `runtime`）
- `DELETE /wunder/admin/channels/accounts/{channel}/{account_id}`
  - 返回：`data.deleted`

- `GET /wunder/admin/channels/bindings`
  - Query：`channel`
  - 返回：`data.items`（binding_id/channel/account_id/peer_kind/peer_id/agent_id/tool_overrides/priority/enabled/created_at/updated_at）
- `POST /wunder/admin/channels/bindings`
  - 入参：`binding_id`（可选）、`channel`、`account_id`、`peer_kind`、`peer_id`、`agent_id`、`tool_overrides`、`priority`、`enabled`
  - 返回：绑定记录
- `DELETE /wunder/admin/channels/bindings/{binding_id}`
  - 返回：`data.deleted`

- `GET /wunder/admin/channels/user_bindings`
  - Query：`channel`、`account_id`、`peer_kind`、`peer_id`、`user_id`、`offset`、`limit`
  - 返回：`data.items`（channel/account_id/peer_kind/peer_id/user_id/created_at/updated_at）与 `data.total`
- `POST /wunder/admin/channels/user_bindings`
  - 入参：`channel`、`account_id`、`peer_kind`、`peer_id`、`user_id`
  - 返回：绑定记录
- `DELETE /wunder/admin/channels/user_bindings/{channel}/{account_id}/{peer_kind}/{peer_id}`
  - 返回：`data.deleted`

- `GET /wunder/admin/channels/sessions`
  - Query：`channel`、`account_id`、`peer_id`、`session_id`、`offset`、`limit`
  - 返回：`data.items`（channel/account_id/peer_kind/peer_id/thread_id/session_id/agent_id/user_id/tts_enabled/tts_voice/metadata/last_message_at/created_at/updated_at）与 `data.total`

- `POST /wunder/admin/channels/test`
  - 入参：`message`（ChannelMessage）
  - 返回：`data.accepted`、`data.session_ids`、`data.outbox_ids`、`data.errors`

补充说明：
- `channels.session_strategy`：`main_thread`/`per_peer`/`hybrid`，控制渠道消息是否进入主线程（默认 `main_thread`）。

#### ChannelAccount.config 字段

- `inbound_token`：入站鉴权 token（可选）
- `outbound_url`：出站投递 URL（可选）
- `outbound_token`：出站鉴权 token（可选）
- `outbound_headers`：出站附加 Headers（对象，可选）
- `timeout_s`：出站超时秒数（可选）
- `allow_peers`/`deny_peers`：允许/禁止的会话列表（可选）
- `allow_senders`/`deny_senders`：允许/禁止的发送者列表（可选）
- `tts_enabled`：是否启用 TTS（可选）
- `tts_voice`：TTS voice 覆盖（可选）
- `tool_overrides`：默认工具覆盖（可选）
- `agent_id`：默认路由 agent_id（可选）

### 4.1.60 `/wunder/channels/*`

- 鉴权：必须携带用户侧 token（`Authorization: Bearer <user_token>`）。
- 用户侧渠道账号仅由当前用户维护，和管理侧渠道账号隔离；管理侧页面仅用于运行态监控。

- `GET /wunder/channels/accounts`
  - Query：`channel`（可选，按渠道过滤，如 `feishu`）
  - 返回：
    - `data.items`：当前用户的渠道账号列表（`channel/account_id/name/status/active/meta/config/raw_config/created_at/updated_at`）
    - `data.supported_channels`：前端可用的渠道类型列表（`channel`）
  - `meta` 关键字段：
    - `configured`：是否已完成可用配置
    - `peer_kind`：默认会话类型（如 `group` / `user`）
    - `receive_group_chat`：是否接收群聊（飞书）

- `POST /wunder/channels/accounts`
  - 用途：新增或更新当前用户的渠道账号。
  - 通用入参：
    - `channel`：渠道类型（必填）
    - `account_id`：账号 ID（更新时必填；创建时可不填）
    - `create_new`：是否强制创建新账号（可选）
    - `account_name`：账号显示名称（可选）
    - `agent_id`：绑定的智能体 ID（可选；传入后默认绑定指向该智能体）
    - `enabled`：是否启用（可选，默认 `true`）
    - `peer_kind`：默认会话类型（非飞书渠道可选）
    - `config`：渠道配置补丁（JSON 对象，可选）
  - 飞书快捷入参：
    - `app_id`、`app_secret`（必填或沿用已有值）
    - `receive_group_chat`（可选，默认 `true`）
    - `domain`（可选，默认 `open.feishu.cn`）
  - 行为说明：
    - 首次创建会自动写入 `inbound_token`。
    - 会自动维护默认绑定（`peer_id="*"`），并按 `peer_kind` / `receive_group_chat` 更新。
    - 传入 `agent_id` 时，默认绑定会同时写入该 `agent_id` 以隔离到指定智能体。
    - 飞书账号保存成功后会以 `long_connection_enabled=true` 参与长连接调度。

- `DELETE /wunder/channels/accounts/{channel}/{account_id}`
  - 删除指定渠道账号，并清理该账号下当前用户的默认绑定与用户绑定映射。
  - 返回：`data.deleted_accounts/deleted_bindings/deleted_user_bindings`。

- `DELETE /wunder/channels/accounts/{channel}`
  - 兼容旧接口。
  - 当该用户在该渠道下仅有 1 个账号时可直接删除；若存在多个账号会返回错误，提示改用带 `account_id` 的删除接口。

- `GET /wunder/channels/bindings`
  - Query：`channel`、`account_id`、`peer_kind`、`peer_id`（均可选）
  - 返回：当前用户可见的绑定列表（含 `binding_id/agent_id/tool_overrides/priority/enabled`）。

- `POST /wunder/channels/bindings`
  - 入参：`channel`、`account_id`、`peer_kind`、`peer_id`，可选 `agent_id/tool_overrides/priority/enabled`。
  - 要求：`account_id` 必须属于当前用户，账号需为启用状态。

- `DELETE /wunder/channels/bindings/{channel}/{account_id}/{peer_kind}/{peer_id}`
  - 删除当前用户在该会话标识下的绑定。

### 4.2 流式响应（SSE）

- 响应类型：`text/event-stream`
- 轮次字段说明：`data.user_round` 为用户轮次，`data.model_round` 为模型轮次。
- 当前 Rust 版会输出 `progress`, `llm_output_delta`, `llm_output`, `context_usage`, `quota_usage`, `tool_call`, `tool_result`, `plan_update`, `question_panel`, `final` 等事件，其余事件待补齐。
- `event: progress`：阶段性过程信息（摘要）
- `event: llm_request`：模型 API 请求体（调试用；默认仅返回基础元信息并标记 `payload_omitted`，开启 `debug_payload` 或日志级别为 debug/trace 时包含完整 payload；若上一轮包含思考过程，将在 messages 中附带 `reasoning_content`；当上一轮为工具调用时，messages 会包含该轮 assistant 原始输出与 reasoning）
- `event: knowledge_request`：知识库检索模型请求体（调试用，包含 `query` 或 `keywords`、`limit`、`embedding_model` 等）
- `event: llm_output_delta`：模型流式增量片段（调试用，`data.delta` 为正文增量，`data.reasoning_delta` 为思考增量，需按顺序拼接）
  - 说明：断线续传回放时可能携带 event_id_start/event_id_end 用于标记合并范围。
- `event: llm_stream_retry`：流式断线重连提示（`data.attempt/max_attempts/delay_s` 说明重连进度，`data.will_retry=false` 或 `data.final=true` 表示已停止重连，`data.reset_output=true` 表示应清理已拼接的输出）
- `event: llm_output`：模型原始输出内容（调试用，`data.content` 为正文，`data.reasoning` 为思考过程，流式模式下为完整聚合结果）
- `event: token_usage`：单轮模型 token 统计（input_tokens/output_tokens/total_tokens；含 `user_round/model_round`）
- `event: context_usage`：上下文占用量统计（data.context_tokens/message_count，含 `user_round/model_round`）
- `event: quota_usage`：额度消耗统计（每次模型调用都会触发；`data.consumed` 为本次消耗次数，`data.daily_quota/used/remaining/date` 为每日额度状态，含 `user_round/model_round`）
- `event: tool_call`：工具调用信息（名称、参数）
- `event: tool_output_delta`：工具执行输出增量（`data.tool`/`data.command`/`data.stream`/`data.delta`）
  - 说明：当前仅内置“执行命令”在本机模式会输出该事件，沙盒执行不流式返回。
- `event: tool_result`：工具执行结果（data.meta.duration_ms/truncated/output_chars/exit_code/policy）
- `event: workspace_update`：工作区变更事件（data.workspace_id/agent_id/tree_version/tool/reason）
- `event: plan_update`：计划看板更新（`data.explanation` 可选，`data.plan` 为步骤数组，包含 `step`/`status`）
- `event: question_panel`：问询面板更新（`data.question` 可选，`data.routes` 为路线数组，包含 `label`/`description`/`recommended`/`selected`）
  - 说明：当 `stop_reason=question_panel` 时会话进入 `waiting`，但后续用户选择仍会立即继续处理（不会触发忙时队列）。
- `event: a2a_request`：A2A 委派请求摘要（endpoint/method/request_id）
- `event: a2a_task`：A2A 任务创建/识别（task_id/context_id）
- `event: a2a_status`：A2A 任务状态更新（state/final）
- `event: a2a_artifact`：A2A 产物更新（artifact name）
- `event: a2a_result`：A2A 任务结束摘要（status/elapsed_ms）
- `event: a2ui`：A2UI 渲染消息（`data.uid`/`data.messages`/`data.content`）
- `event: compaction`：上下文压缩信息（原因/阈值/重置策略/执行状态；压缩请求使用独立 system 提示词、历史消息合并为单条 user 内容，压缩后摘要以 user 注入）
- `event: final`：最终回复（`data.answer`/`data.usage`/`data.stop_reason`）
  - `stop_reason` 取值：`model_response`（模型直接回复）、`final_tool`（最终回复工具）、`a2ui`（A2UI 工具）、`question_panel`（等待问询面板选择）、`max_rounds`（达到最大轮次兜底）、`unknown`（兜底）
- `event: error`：错误信息（包含错误码与建议）
- SSE 会附带 `id` 行，代表事件序号，可用于客户端排序或去重。
- 当 SSE 队列满时事件会写入 `stream_events`，流式通道会回放补齐。

事件 payload 示例：

```json
{
  "type": "progress",
  "timestamp": "2025-12-24T08:30:00Z",
  "session_id": "u_1234_20251224",
  "data": {
    "stage": "plan",
    "summary": "已完成需求拆解，准备执行工具调用。"
  }
}
```

### 4.1.61 `/wunder/admin/gateway/*`

- `GET /wunder/admin/gateway/status`
  - 返回：`enabled/protocol_version/state_version/connections/nodes_total/nodes_online`
- `GET /wunder/admin/gateway/presence`
  - 返回：`data.items`（连接快照）
- `GET /wunder/admin/gateway/clients`
  - Query：`status`
  - 返回：`data.items`（connection_id/role/user_id/node_id/scopes/caps/commands/status/connected_at/last_seen_at）
- `GET /wunder/admin/gateway/nodes`
  - Query：`status`
  - 返回：`data.items`（node_id/name/device_fingerprint/status/caps/commands/permissions/metadata/created_at/updated_at/last_seen_at）
- `POST /wunder/admin/gateway/nodes`
  - 入参：`node_id`（可选）、`name`、`status`、`device_fingerprint`、`metadata`
  - 返回：节点记录
- `GET /wunder/admin/gateway/node_tokens`
  - Query：`node_id`、`status`
  - 返回：`data.items`（token/node_id/status/created_at/updated_at/last_used_at）
- `POST /wunder/admin/gateway/node_tokens`
  - 入参：`node_id`（可选）、`status`（可选，默认 active）
  - 返回：节点 token 记录（包含 `token`）
- `DELETE /wunder/admin/gateway/node_tokens/{token}`
  - 返回：`data.removed`
- `POST /wunder/admin/gateway/invoke`
  - 入参：`node_id`、`command`、`args`（可选）、`timeout_s`（可选）、`metadata`（可选）
  - 返回：`data.ok/data.payload/data.error`

### 4.2.2 WebSocket 流式响应（可选）

- 说明：聊天端默认传输由 `server.chat_stream_channel` 控制（默认 `ws`，可切到 `sse`）；无论通道如何切换，事件语义与字段均保持一致（`event/id/data`）。
- Endpoint（用户侧）：`/wunder/chat/ws`
- Endpoint（统一入口）：`/wunder/ws`
- 鉴权：
  - 浏览器：推荐 `Sec-WebSocket-Protocol` 传 token（`wunder`, `wunder-auth.<token>`），Query `access_token` 仅兼容
  - 非浏览器客户端：`Authorization: Bearer <token>`
- 消息格式：JSON Envelope，服务端推送 `type=event`，payload 内含 `event/id/data`
- 应用层握手：连接建立后服务端发送 `ready`（含 `protocol`/`policy`）；客户端建议先发送 `connect` 携带协议版本与客户端信息，不兼容会返回 `error` 并关闭连接；未发送 `connect` 时按默认协议版本处理
- 慢客户端告警：当客户端消费过慢导致队列压力时，服务端会发送 `event=slow_client`，前端可提示用户触发 `resume`
- 多路复用：同一连接可并发多个请求，需设置 `request_id`；服务端 `event/error` 会回传对应 `request_id`
- `type=error` 统一错误载荷字段：`code`/`message`/`status`/`hint`/`trace_id`/`timestamp`。
- 断线续传：客户端发送 `resume` + `after_event_id`，服务端从 `stream_events` 回放并继续推送
- 实时订阅：客户端发送 `watch` + `after_event_id`，服务端持续推送会话流事件（直到取消或断线）
- 详细协议与节点说明：见 `docs/方案/WebSocket-Transport.md`

### 4.2.3 Gateway WebSocket Control Plane

- Endpoint: `/wunder/gateway/ws`
- Roles: `operator`/`node`/`channel`, declared via `connect.role`.
- Auth: `connect.params.auth.token` maps to `gateway.auth_token`; nodes can additionally provide `auth.node_token` issued by admin APIs.
- Handshake requirement: first frame must be `type=req` + `method=connect`, and handshake must finish within `10s`, otherwise server returns `HANDSHAKE_TIMEOUT` and closes.
- Payload validation: gateway envelope and `connect.params` both reject unknown fields; `req` requires non-empty `id/method`; `res` requires non-empty `id` and explicit `ok`.
- Origin validation: when `Origin` header exists, it must be same-host (loopback exception) or be explicitly listed in `gateway.allowed_origins`.
- Trusted proxy behavior: `X-Forwarded-For/X-Real-IP` is trusted only when remote address is in `gateway.trusted_proxies`; resolved client IP is returned as `hello-ok.payload.client_ip`.
- Server `hello-ok` payload includes `protocol/policy/presence/stateVersion/server_time/client_ip`.
- Runtime events: gateway emits periodic `gateway.tick` (policy interval) and `gateway.health` (every 60s with `connections/nodes_online/stateVersion`).
- Error observability: when `res.ok=false`, `error` includes `code/message/status/hint/trace_id/timestamp` for cross-end tracing.
- Slow client governance: presence/event fan-out uses non-blocking send; repeatedly backpressured clients are evicted and a fresh `gateway.presence.update` is broadcast.
- `node.invoke` response correlation is strictly validated by `request_id + connection_id + node_id` to prevent cross-connection spoofing.

### 4.3 非流式响应

- 返回 JSON：
  - `session_id`
  - `answer`
  - `usage`（可选）
  - `stop_reason`（可选，停止原因，同 `event: final`）
  - `uid`（可选，A2UI Surface 标识）
  - `a2ui`（可选，A2UI 消息数组）

### 4.4 工具协议（EVA 风格）

- `tool_call_mode=tool_call`（默认）：模型以 `<tool_call>...</tool_call>` 包裹 JSON 调用工具，工具结果以 `tool_response: ` 前缀的 user 消息回填。
- `tool_call_mode=function_call`：模型通过 OpenAI 风格 `tool_calls/function_call` 返回工具调用，工具结果以 role=tool + tool_call_id 回填。
- `function_call` 模式下系统提示词不再注入工具清单与工具调用引导，工具清单仅通过请求 `tools` 传入；技能提示词仍会注入。
- `function_call` 模式需要在后续请求中携带历史的 assistant `tool_calls` 与 role=tool/tool_call_id 结果；wunder 会将其写入对话历史并自动回填。
- 系统提示词按职能模块拼装（角色/安全/产品/编程/运行环境/协议/工程师信息）；运行环境模块按 `server.mode` 选择：`api/sandbox` 使用 server 运行模块，`cli/desktop` 使用本地运行模块（无固定依赖清单）。
- 提示词模板按语言优先读取 `prompts/zh` 或 `prompts/en`；英文模式下内置工具名在提示词与 `/wunder/tools` 输出中优先英文别名。
- JSON 结构：`{"name":"工具名","arguments":{...}}`。
- 工具结果以 `tool_response: ` 前缀的 user 消息回填给模型，用于下一轮判断（`tool_call` 模式）。
- A2A 服务工具由管理员在 `/wunder/admin/a2a` 配置，启用后以 `a2a@service` 形式对模型可用；`tool_call` 模式下注入系统提示词。
- 命令执行白名单由 `security.allow_commands` 控制，支持 `*` 放开全部命令。
- 高风险命令在 `security.exec_policy_mode=enforce` 时需显式审批（tool args 支持 `approved=true`/`approval_key`），审批结果会在会话内短 TTL 缓存。
- 执行命令支持 `workdir` 指定工作目录（仅工作区内相对路径），`timeout_s` 可选。
- 系统提示词中工作目录展示为 `/workspaces/<user_id>/`，实际工作区根为 `workspace.root/<user_id>`。
- 文件类内置工具默认仅允许访问工作区，可通过 `security.allow_paths` 放行白名单目录（允许绝对路径）。
- MCP 工具调用形式为 `server@tool`，技能工具按管理员启用的名称暴露。

示例：

```text
<tool_call>
{"name":"列出文件","arguments":{"path":"."}}
</tool_call>
```

### 4.5 存储说明

- 系统日志、对话历史、工具日志、产物索引、监控记录、会话锁与溢出事件统一写入数据库（优先 PostgreSQL，可选 SQLite）。
- 存储后端由 `storage.backend` 控制：`auto`（默认，优先 PostgreSQL，不可用则自动降级 SQLite）、`postgres`、`sqlite`。
- SQLite 使用 `storage.db_path`，PostgreSQL 使用 `storage.postgres.dsn`（支持 `${VAR:-default}` 环境变量占位符）。
- PostgreSQL 连接池大小由 `storage.postgres.pool_size` 控制（默认 64，可通过 `WUNDER_POSTGRES_POOL_SIZE` 覆盖）。
- 旧版 `data/historys/` 已停用，不再作为主存储。

### 4.6 沙盒服务 API

> 说明：共享沙盒服务由第二个 wunder 容器提供，默认用于沙盒模式。

#### 4.6.1 `GET /health`

- 返回（JSON）：
  - `ok`：布尔，服务健康状态

#### 4.6.2 `POST /sandboxes/execute_tool`

- 入参（JSON）：
  - `user_id`：字符串，用户唯一标识
  - `session_id`：字符串，可选，会话标识
  - `tool`：字符串，内置工具名称
  - `args`：对象，工具参数
  - `workspace_root`：字符串，容器内工作区根路径
  - `allow_paths`：字符串数组，允许访问的白名单路径
  - `deny_globs`：字符串数组，拒绝访问的通配规则
  - `allow_commands`：字符串数组，允许执行的命令白名单
  - `container_root`：字符串，容器内项目根路径
  - `network`：字符串，容器网络模式
  - `readonly_rootfs`：布尔，是否只读根文件系统
  - `idle_ttl_s`：整数，空闲回收秒数
  - `resources`：对象（cpu/memory_mb/pids）
- 返回（JSON）：
  - `ok`：布尔，工具是否成功
  - `data`：对象，工具返回数据
  - `error`：字符串，错误信息
  - `debug_events`：数组，工具内部调试事件

#### 4.6.3 `POST /sandboxes/release`

- 入参（JSON）：
  - `user_id`：字符串，用户唯一标识
  - `session_id`：字符串，可选，会话标识
- 返回（JSON）：
  - `ok`：布尔，是否释放成功
  - `message`：字符串，释放结果说明

#### 4.6.4 运行说明

- 共享沙盒服务不创建子容器，依赖同一镜像运行与工作区挂载即可。
- docker compose 内网部署推荐使用容器内部 DNS（默认 `http://sandbox:9001`）直连沙盒且无需对外暴露 9001 端口；运行时会优先读取 `WUNDER_SANDBOX_ENDPOINT` 并在常见地址间自动回退以降低 IP 配置失败概率。
- 如需在 ptc/Matplotlib 或其他字体依赖中渲染中文，建议将仓库 `fonts/` 挂载到 `/usr/share/fonts/wunder`，并使用 `FONTCONFIG_FILE=/app/config/fonts.conf`、`XDG_CACHE_HOME=/workspaces/.cache`、`MATPLOTLIBRC=/app/config/matplotlibrc` 与 `MPLCONFIGDIR=/workspaces/.matplotlib`（docker compose 已默认配置）。

### 4.7 A2A 标准接口

#### 4.7.1 `GET /.well-known/agent-card.json`

- 说明：A2A AgentCard 发现入口（公开访问）。
- 返回（JSON）：AgentCard 元数据（protocolVersion、supportedInterfaces、skills、capabilities、tooling 等）。
- 备注：`tooling` 汇总内置工具/MCP/A2A/知识库工具清单，用于客户端能力展示，默认仅保留名称/描述，不包含参数 schema。

#### 4.7.2 `GET /a2a/agentCard`

- 说明：返回基础 AgentCard（与 `/.well-known/agent-card.json` 一致），用于兼容只探测 `/a2a/*` 路径的客户端。
- 鉴权：需携带 API Key。

#### 4.7.3 `GET /a2a/extendedAgentCard`

- 说明：返回扩展 AgentCard（相比基础版可能补充 `documentationUrl` 等字段，仍保持轻量清单结构）。
- 鉴权：需携带 API Key。

#### 4.7.4 `POST /a2a`

- 类型：JSON-RPC 2.0
- 说明：A2A 标准方法入口，支持 `SendMessage`、`SendStreamingMessage`、`GetTask`、`ListTasks`、`CancelTask`、`GetExtendedAgentCard`。
- 鉴权：需携带 API Key。
- JSON-RPC 请求结构：
  - `jsonrpc`: 固定 `2.0`
  - `id`: 请求标识
  - `method`: A2A 方法名
  - `params`: A2A 参数对象
- 流式返回：
  - `SendStreamingMessage` 返回 `text/event-stream`
  - SSE data 内容为 A2A StreamResponse（`task`/`statusUpdate`/`artifactUpdate`）
  - `statusUpdate.final=true` 表示任务结束
- 错误观测：JSON-RPC 错误除标准 `error.code/error.message` 外，`error.data` 统一补充 `error_code/status/hint/trace_id/timestamp`，并保留业务字段（如 `taskId/quota/detail`）。

## 5. 附录：辅助脚本

- `scripts/update_feature_log.py`：按分类写入 `docs/功能迭代.md`（支持 `--type/--scope`），默认使用 UTF-8 BOM 避免乱码。

### 4.8 蜂巢与蜂群接口（2026-02增补）

#### 4.8.1 `/wunder/hives`
- 方法：`GET/POST`
- `GET`：查询当前用户蜂巢列表，支持 `include_archived` 参数。
- `POST`：创建蜂巢，`name` 必填，`description` 可选；支持 `copy_from_hive_id` 从已有蜂巢复制应用。

#### 4.8.2 `/wunder/hives/{hive_id}`
- 方法：`PATCH`
- 说明：更新蜂巢基础信息，支持 `name/description/status/is_default` 字段。

#### 4.8.3 `/wunder/hives/{hive_id}/summary`
- 方法：`GET`
- 说明：返回蜂巢聚合指标（应用数、运行中会话、近期成功率、平均耗时等）。
- 参数：`lookback_minutes`，默认 60 分钟。

#### 4.8.4 `/wunder/hives/{hive_id}/agents`
- 方法：`POST`
- 说明：将一组智能体迁移到目标蜂巢。
- 请求体：`agent_ids: string[]`。

#### 4.8.5 `/wunder/chat/team_runs`
- 方法：`GET/POST`
- `GET`：按用户查询 TeamRun，支持 `hive_id/parent_session_id/offset/limit`。
- `POST`：创建 TeamRun。
  - `parent_session_id` 必填。
  - `hive_id` 可选，不传时按父会话推导。
  - `strategy/merge_policy/timeout_s` 可选。
  - `tasks[]` 必填，元素包含 `agent_id` 与可选 `target_session_id/priority`。
  - 创建阶段先落库为 `preparing`，待 `tasks[]` 全部写入后再切换为 `queued` 并入队，避免 Runner 并发扫描抢跑导致任务遗漏。

#### 4.8.6 `/wunder/chat/team_runs/{team_run_id}`
- 方法：`GET`
- 说明：查询单个 TeamRun 详情及其 TeamTask 列表。

#### 4.8.7 `/wunder/chat/team_runs/{team_run_id}/cancel`
- 方法：`POST`
- 说明：取消进行中的 TeamRun，并将状态更新为 `cancelled`。

#### 4.8.8 `/wunder/chat/sessions/{session_id}/team_runs`
- 方法：`GET`
- 说明：按会话查询 TeamRun，支持 `hive_id/offset/limit`。

#### 4.8.9 `/wunder/admin/team_runs`
- 方法：`GET`
- 说明：管理端查询 TeamRun，支持按 `user_id/hive_id/parent_session_id` 过滤。

#### 4.8.10 `/wunder/admin/team_runs/{team_run_id}`
- 方法：`GET`
- 说明：管理端查询单个 TeamRun + TeamTask 详情。

#### 4.8.11 `/wunder/admin/hives/{hive_id}/team_runs`
- 方法：`GET`
- 说明：管理端按蜂巢查询 TeamRun，支持 `user_id` 过滤。

#### 4.8.13 `/wunder/admin/sim_lab/projects`
- 方法：`GET`
- 说明：返回模拟测试项目列表与默认参数，当前内置 `swarm_flow` 项目。
- 鉴权：管理员令牌（Bearer）。

#### 4.8.14 `/wunder/admin/sim_lab/runs`
- 方法：`POST`
- 说明：执行模拟测试任务，当前固定并行执行（`mode` 字段仅保留兼容，不影响执行模式）。
- 鉴权：管理员令牌（Bearer）。
- 请求体示例：
```json
{
  "run_id": "simlab_20260210_ab12cd",
  "projects": ["swarm_flow"],
  "keep_artifacts": false,
  "options": {
    "swarm_flow": {
      "workers": 100,
      "max_wait_s": 180,
      "mother_wait_s": 30,
      "poll_ms": 120,
      "keep_artifacts": false
    }
  }
}
```
- 参数说明：`run_id` 可选，建议前端传入用于后续停止；未传时服务端自动生成。
- 运行前置：服务端会确保模拟账号 `wunder-sim` 存在；每次运行会重置该账号下历史会话与智能体应用，并按 `workers` 重建工蜂应用（容器 `1..10` 随机分配），随后由母蜂发起真实蜂群链路。
- 响应：`data` 返回 `run_id/mode/wall_time_s/project_total/project_success/project_failed/projects[]`，其中 `projects[]` 含每个项目的执行状态、耗时、报告与错误信息。

#### 4.8.15 `/wunder/admin/sim_lab/runs/{run_id}/status`
- 方法：`GET`
- 说明：查询指定模拟测试运行是否仍在服务端活动中（`active=true|false`）。
- 鉴权：管理员令牌（Bearer）。

#### 4.8.16 `/wunder/admin/sim_lab/runs/{run_id}/cancel`
- 方法：`POST`
- 说明：停止指定模拟测试运行，服务端会对对应会话发起取消并等待收敛。
- 鉴权：管理员令牌（Bearer）。

#### 4.8.12 错误码
- `SWARM_HIVE_UNRESOLVED`：蜂巢不存在或无法解析。
- `SWARM_HIVE_DENIED`：目标智能体不在当前蜂巢作用域。
- `SWARM_POLICY_BLOCKED`：触发策略限制（并发、任务数、参数校验）。
- `SWARM_RUN_TIMEOUT`：蜂群运行超时。

## 单蜂巢 API 变更（2026-02）

- 已移除：`/wunder/hives`、`/wunder/hives/{hive_id}`、`/wunder/hives/{hive_id}/summary`、`/wunder/hives/{hive_id}/agents`。
- `/wunder/agents` 不再按 `hive_id` 参数筛选，始终返回当前用户的单蜂巢智能体列表。
- `/wunder/chat/team_runs` 与 `/wunder/chat/sessions/{session_id}/team_runs` 不再使用 `hive_id` 过滤参数。
- 返回体中的 `hive_id` 保留为兼容字段，固定为 `default`。

## wunder-cli 本地命令入口

> 说明：wunder-cli 为本地命令行入口，不走 HTTP 路由，但复用与 /wunder 相同的 orchestrator/tool/mcp/skills 核心链路。

- 默认无子命令在 TTY 进入 codex 风格 TUI；有 PROMPT 时执行单次任务。
- `wunder-cli ask <PROMPT>`：单次提问。
- `wunder-cli chat [PROMPT]`：经典行式交互会话（非 TUI 兜底）。
- `wunder-cli resume [SESSION_ID] [PROMPT] [--last]`：恢复会话（TTY 默认进入 TUI，非 TTY 回退行式交互）。
- `wunder-cli tool run <name> --args <json>`：直接工具调用。
- `wunder-cli exec|e <command...>`：命令执行快捷入口。
- `wunder-cli mcp list|get|add|remove|enable|disable`：本地 MCP 配置管理。
- `wunder-cli skills list|enable|disable`：本地 skills 启用状态管理。
- `wunder-cli config show|set-tool-call-mode`：查看/设置运行配置。
- `wunder-cli doctor`：运行时环境诊断。
- 无子命令且终端可用时，默认进入 codex 风格 TUI（顶部状态栏 + 会话区 + 命令提示 + 输入区）。
- 默认读取 `--config` 指定或 `config/wunder.yaml`；若 CLI 未找到仓库配置，会自动生成 `WUNDER_TEMP/config/wunder.base.yaml` 作为基础配置。

- 交互态内置 codex 风格 slash 命令：
  - `/help`：输出命令帮助清单（含描述）。
  - `/status`：输出会话状态（session/model/tool_call_mode/workspace/db）。
  - `/model [name]`：查看当前模型，或切换默认模型。
  - `/tool-call-mode <tool_call|function_call> [model]`（别名 `/mode`）：切换调用协议。
  - `/config`：TUI 与 `chat` 行式模式统一为向导式配置（`base_url -> api_key -> model -> max_context`），并支持 `/config <base_url> <api_key> <model> [max_context]` 一行配置。
  - `/config show`：输出当前运行配置。
  - `/new` / `/session` / `/system` / `/mouse` / `/exit`：会话与系统提示词控制（`/session` 输出上下文与调用统计，`/mouse` 切换滚轮/复制模式）。
- TUI 顶部状态栏突出 `xx% context left`，隐藏低价值 mode/state 字段；会话区支持 PgUp/PgDn/Home/End 与鼠标滚轮滚动。
- TUI 多行编辑体验已对齐 codex 快捷键：`Shift+Enter`/`Ctrl+J` 换行，`Ctrl+B/F` 字符移动，`Alt+B/F/Left/Right` 按词导航，`Ctrl+W`/`Alt+Backspace`/`Alt+Delete` 按词删除。
- 输入区会根据视口宽度自动折行，并在按键长按（`KeyEventKind::Repeat`）时保持光标移动流畅。
- 鼠标模式可通过 `F2` 或 `/mouse [scroll|select]` 切换：`scroll` 启用滚轮滚动，`select` 关闭捕获以便直接框选复制。
- 对于未识别的 `/xxx` 输入，CLI 会提示 unknown command 并引导 `/help`。
- CLI 提示词优先读取 `prompts/zh|en` 文件（可通过 `WUNDER_PROMPTS_ROOT` 指定根目录），缺失时自动回退二进制内嵌模板。
- 流式偏移读取在空会话下回落为 `0`，不再因 `MAX(event_id)=NULL` 触发告警。

## wunder-desktop 本地桥接接口约定（M0/M4 已落地）

> wunder-desktop 复用既有 `/wunder` 协议，不引入新业务协议版本；差异主要在 Tauri 桌面壳、运行时引导、免登录注入与本地路由约束。

- 默认地址：`http://127.0.0.1:18000`（支持 `--host/--port` 覆盖，`--port 0` 自动分配）。
- API 基址：`http://<host>:<port>/wunder`
- WS 基址：`ws://<host>:<port>/wunder/chat/ws`
- 默认启动模式：Tauri GUI；`--bridge-only` 用于诊断与无窗口运行。

### desktop 运行时引导接口

- `GET /config.json`
  - 用途：前端运行时配置注入。
  - 返回字段：`api_base`、`ws_base`、`token`、`desktop_token`、`user_id`、`workspace_root`、`mode`、`remote_enabled`、`remote_connected`、`remote_server_base_url`、`remote_error`。
- `GET /wunder/desktop/bootstrap`
  - 用途：桌面启动器/诊断查看完整运行时信息。
  - 返回字段：`web_base`、`api_base`、`ws_base`、`token`、`desktop_token`、`user_id`、`workspace_root`、`temp_root`、`settings_path`、`frontend_root`、`remote_enabled`、`remote_connected`、`remote_server_base_url`、`remote_error`。

### desktop 本地设置接口

- `GET /wunder/desktop/settings`
  - 用途：读取 desktop 本地设置快照。
  - 返回字段：
    - `workspace_root`：容器 1 的默认工作目录
    - `container_roots`：数组，元素 `{ container_id, root }`
    - `language` / `supported_languages`
    - `llm`（`default` + `models`）
    - `remote_gateway`（服务端连接配置，仅含 `enabled` 与 `server_base_url`）
    - `updated_at`
- `PUT /wunder/desktop/settings`
  - 用途：更新 desktop 本地设置并同步到运行态。
  - 请求字段（均可选）：
    - `workspace_root`：字符串，容器 1 工作目录
    - `container_roots`：数组，元素 `{ container_id, root }`
    - `language`：字符串，例如 `zh-CN` / `en-US`
    - `llm`：对象（`default` + `models`）
    - `remote_gateway`：对象（仅 `enabled` 与 `server_base_url`）
  - 行为约束：
    - 容器 id 会归一化到 `1..10`；容器 1 默认指向 `WUNDER_WORK`。
    - 相对路径按桌面程序目录解析，并在保存时自动创建目录。
    - 更新后会同步写入 `WUNDER_TEMPD/config/desktop.settings.json`、运行中的 `ConfigStore` 与 `WorkspaceManager`。

### desktop 远端接入（一期）

- 当 `remote_gateway.enabled=true` 且 `server_base_url` 可解析时，desktop 会把 runtime 的 `api_base/ws_base` 切到远端 `wunder-server`。
- 前端在系统设置页仅需填写服务端地址；点击“接入服务端”后会跳转 `/login`，按常规注册/登录流程鉴权。
- desktop **不会自动创建账号或自动登录**；用户需按常规流程调用：
  - `POST <server_base_url>/wunder/auth/register` 注册，或
  - `POST <server_base_url>/wunder/auth/login` 登录。
- 远端模式下：
  - runtime 的 `token` 默认为空；
  - 本地桌面设置接口继续使用 `desktop_token` 独立鉴权。
- 若远端地址无效：
  - 自动回退到本地模式继续可用；
  - 失败原因写入 runtime 的 `remote_error` 字段。

### Tauri command（桌面窗口内）

- `desktop_runtime_info`
  - 来源：`wunder-desktop/main.rs` 的 `#[tauri::command]`。
  - 返回：与 `/wunder/desktop/bootstrap.data` 同构的 runtime 快照。
  - 用途：桌面 UI 在不发 HTTP 请求时获取 runtime 信息。

### desktop 鉴权与免登录约定

- 使用 desktop 本地 token（启动时生成并持久化到 `WUNDER_TEMPD/config/desktop.settings.json`）。
- token guard 仅作用于 `/wunder/**` 业务接口；静态页与引导接口可无 token 访问。
- 桌面模式会在 `index.html` 注入 runtime，并写入 `localStorage.access_token`；用户侧无需登录流程即可进入 `/desktop/home`。
- 远端模式下不再自动写入远端 token；用户登录/注册成功后由正常鉴权流程写入 `localStorage.access_token`，本地设置接口仍使用 `desktop_token` 独立鉴权。
- 支持从以下位置携带 token：
  - `x-api-key`
  - `Authorization: Bearer <token>`
  - `sec-websocket-protocol` 中的 `wunder-auth.<token>`
  - 查询参数 `access_token` / `api_key`

### desktop 会话请求扩展

- `POST /wunder/chat/sessions/{id}/messages`
  - 新增可选字段：`tool_call_mode`（`tool_call` 或 `function_call`）。
- `WS /wunder/chat/ws` 的 `start` payload
  - 新增可选字段：`tool_call_mode`（`tool_call` 或 `function_call`）。
- 当携带 `tool_call_mode` 时，服务端会在本次请求中生成 `config_overrides.llm.models.<default>.tool_call_mode`，仅影响当前轮请求。

### desktop 暴露路由范围（当前）

- 包含：`auth/chat/chat_ws/core/core_ws/external_links/workspace/user_tools/user_agents/user_channels/mcp/temp_dir`
- 不包含：`admin/channel/gateway/cron/a2a` 等管理或多租户路由

### 前端托管约定

- 默认托管目录：`frontend/dist`（支持 `--frontend-root` 自定义）。
- `GET /`、`GET /index.html`、`GET /{*path}` 回退到前端 `index.html`。
- 服务端在 `index.html` 注入 `window.__WUNDER_DESKTOP_RUNTIME__`，并写入 `localStorage.access_token`，确保用户端前端可直接进入会话。

### 启动期目录与持久化

- `WUNDER_TEMPD/wunder_desktop.sqlite3`
- `WUNDER_TEMPD/config/wunder.override.yaml`
- `WUNDER_TEMPD/config/desktop.settings.json`
- `WUNDER_TEMPD/user_tools/*`
- `WUNDER_TEMPD/vector_knowledge/*`


