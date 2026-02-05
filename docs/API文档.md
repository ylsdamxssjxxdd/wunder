# wunder API 文档

## 4. API 设计

### 4.0 实现说明

- 接口实现基于 Rust Axum，路由拆分在 `src/api` 的 core/admin/workspace/user_tools/a2a 模块。
- 运行与热重载环境建议使用 `Dockerfile.rust` + `docker-compose-x86.yml`/`docker-compose-arm.yml`。
- MCP 服务容器：`wunder_mcp` 用于运行 `mcp_server/` 下的 FastMCP 服务脚本，默认以 streamable-http 暴露端口，人员数据库连接通过 `mcp_server/mcp_config.json` 的 `database` 配置。
- MCP 配置文件：`mcp_server/mcp_config.json` 支持集中管理人员数据库配置，可通过 `MCP_CONFIG_PATH` 指定路径，数据库配置以配置文件为准。
- 多数据库支持：在 `mcp_config.json` 的 `database.targets` 中配置多个数据库（MySQL/PostgreSQL），默认使用 `default_key`，需要切换目标可调整 `default_key` 或部署多个 MCP 实例。
- 单库类型切换：设置 `database.db_type=mysql|postgres`，或在多库配置中为每个目标指定 `type/engine` 或 DSN scheme。
- 知识库 MCP：`kb_query` 可选配置（`mcp_config.json` 的 `knowledge`），向量知识库检索不依赖 RAGFlow MCP。
- 向量知识库使用 Weaviate，连接参数位于 `vector_store.weaviate`（url/api_key/timeout_s/batch_size）。
- docker compose 默认使用命名卷 `wunder_postgres` 保存 PostgreSQL 数据，避免绑定到 `data/` 目录。
- 沙盒服务：独立容器运行 `wunder-server` 的 `sandbox` 模式（`WUNDER_SERVER_MODE=sandbox`），对外提供 `/sandboxes/execute_tool` 与 `/sandboxes/release`，由 `WUNDER_SANDBOX_ENDPOINT` 指定地址。
- 工具清单与提示词注入复用统一的工具规格构建逻辑，确保输出一致性（`tool_call` 模式）；`function_call` 模式不注入工具提示词，工具清单仅用于 tools 协议。
- 配置分层：基础配置为 `config/wunder.yaml`（`WUNDER_CONFIG_PATH` 可覆盖），管理端修改会写入 `data/config/wunder.override.yaml`（`WUNDER_CONFIG_OVERRIDE_PATH` 可覆盖）。
- 环境变量：建议使用仓库根目录 `.env` 统一管理常用变量，docker compose 默认读取（如 `WUNDER_HOST`/`WUNDER_PORT`/`WUNDER_API_KEY`/`WUNDER_POSTGRES_DSN`/`WUNDER_SANDBOX_ENDPOINT`）。
- 鉴权：管理员接口使用 `X-API-Key` 或 `Authorization: Bearer <api_key>`（配置项 `security.api_key`），用户侧接口使用 `/wunder/auth` 颁发的 `Authorization: Bearer <user_token>`。
- 默认管理员账号为 admin/admin，服务启动时自动创建且不可删除，可通过用户管理重置密码。
- 用户端请求可省略 `user_id`，后端从 Token 解析；管理员接口可显式传 `user_id` 以指定目标用户。
- 模型配置新增 `model_type=llm|embedding`，向量知识库依赖 embedding 模型调用 `/v1/embeddings`。
- 用户端前端默认入口为 `/app/chat` 聊天页，功能广场入口为 `/home`（实际路由 `/app/home`），外链入口由 `frontend/src/config/external-links.js` 统一管理。
- 当使用 API Key/管理员 Token 访问 `/wunder`、`/wunder/chat`、`/wunder/workspace`、`/wunder/user_tools` 时，`user_id` 允许为“虚拟用户”，无需在 `user_accounts` 注册，仅用于线程/工作区/工具隔离。
- 注册用户按单位层级分配默认每日额度（一级/二级/三级/四级 = 10000/5000/1000/100），每日 0 点重置；额度按每次模型调用消耗，超额返回 429，虚拟用户不受限制。
- 管理员用户执行请求不受额度、会话锁、历史裁剪、监控裁剪、模型/工具超时与历史清理限制，适合长期运行任务。
- A2A 接口：`/a2a` 提供 JSON-RPC 2.0 绑定，`SendStreamingMessage` 以 SSE 形式返回流式事件，AgentCard 通过 `/.well-known/agent-card.json` 暴露。
- 多语言：Rust 版默认从 `config/i18n.messages.json` 读取翻译（可用 `WUNDER_I18N_MESSAGES_PATH` 覆盖）；`/wunder/i18n` 提供语言配置，响应包含 `Content-Language`。
- Rust 版现状：MCP 服务与工具发现/调用已落地（rmcp + streamable-http）；Skills/知识库转换与数据库持久化仍在迁移，相关接口以轻量结构返回。

### 4.1 `/wunder` 请求

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：字符串，用户唯一标识
  - `question`：字符串，用户问题
  - `tool_names`：字符串列表，可选，指定启用的内置工具/MCP/技能名称
  - `skip_tool_calls`：布尔，可选，是否忽略模型输出中的工具调用并直接结束（默认 false）
  - `stream`：布尔，可选，是否流式输出（默认 true）
  - `debug_payload`：布尔，可选，调试用，开启后会保留模型请求体用于事件与日志记录（默认 false）
  - `session_id`：字符串，可选，指定会话标识
  - `agent_id`：字符串，可选，智能体应用 id（用于附加提示词与工作区隔离）
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
- `子智能体控制` 的 `send` 支持 `timeoutSeconds` 等待回复，`spawn` 支持 `runTimeoutSeconds` 等待完成并返回 `reply/elapsed_s`。
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
- 事件列表会按 `observability.monitor_event_limit` 保留最近 N 条，<= 0 表示不截断。
  - 管理员会话不受事件数量与字段裁剪限制。
  - 字符串字段会按 `observability.monitor_payload_max_chars` 截断（<= 0 表示不截断）。
  - `llm_request` 事件仅保存 `payload_summary` 与 `message_count`，不保留完整请求体。
  - `observability.monitor_drop_event_types` 可过滤不持久化的事件类型。
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

- 说明：所有 workspace 接口支持可选 `agent_id`，用于按智能体应用划分工作区；未传或为空时使用默认用户工作区。
- 方法：`GET`
- 入参（Query）：
  - `user_id`：用户唯一标识
  - `agent_id`：智能体应用 id（可选，未传或为空表示默认工作区）
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
- 吞吐量/性能/评估：`/wunder/admin/throughput/*`、`/wunder/admin/performance/sample`、`/wunder/admin/evaluation/*`。
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
- 错误返回：`detail.message`

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

### 4.1.55 `/wunder/chat/*`

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
  - 返回：`data.total`、`data.items`（id/name/description/system_prompt/tool_names/access_level/is_shared/status/icon/created_at/updated_at）
- `GET /wunder/agents/shared`：共享智能体列表
  - 返回：`data.total`、`data.items`（同上）
- `GET /wunder/agents/running`：当前运行中的智能体会话锁 + 问询面板待选择状态
  - 返回：`data.total`、`data.items`（agent_id/session_id/updated_at/expires_at/state/pending_question/is_default）
  - `is_default`：表示通用聊天（无 agent_id 的默认入口会话）
  - `state`：`running` | `waiting`，`pending_question` 表示存在待选择问询面板
- `POST /wunder/agents`：创建智能体
  - 入参（JSON）：`name`（必填）、`description`（可选）、`system_prompt`（可选）、`tool_names`（可选）、`is_shared`（可选）、`status`（可选）、`icon`（可选）
  - 返回：`data`（同智能体详情）
- `GET /wunder/agents/{agent_id}`：智能体详情
  - 返回：`data`（同智能体详情）
- `PUT /wunder/agents/{agent_id}`：更新智能体
  - 入参（JSON）：`name`/`description`/`system_prompt`/`tool_names`/`is_shared`/`status`/`icon`（可选）
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

### 4.1.59 `/wunder/admin/channels/*`

- `GET /wunder/admin/channels/accounts`
  - Query：`channel`、`status`
  - 返回：`data.items`（channel/account_id/config/status/created_at/updated_at）
- `POST /wunder/admin/channels/accounts`
  - 入参：`channel`、`account_id`、`config`、`status`（可选）
  - 返回：账号记录
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

- `GET /wunder/admin/channels/sessions`
  - Query：`channel`、`account_id`、`peer_id`、`session_id`、`offset`、`limit`
  - 返回：`data.items`（channel/account_id/peer_kind/peer_id/thread_id/session_id/agent_id/user_id/tts_enabled/tts_voice/metadata/last_message_at/created_at/updated_at）与 `data.total`

- `POST /wunder/admin/channels/test`
  - 入参：`message`（ChannelMessage）
  - 返回：`data.accepted`、`data.session_ids`、`data.outbox_ids`、`data.errors`

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

### 4.2.2 WebSocket 流式响应（默认）

- 说明：WebSocket 作为默认传输层，不改变事件语义与字段；事件仍与 SSE 保持一致（`event/id/data`）。
- Endpoint（用户侧）：`/wunder/chat/ws`
- Endpoint（统一入口）：`/wunder/ws`
- 鉴权：
  - 浏览器：推荐 `Sec-WebSocket-Protocol` 传 token（`wunder`, `wunder-auth.<token>`），Query `access_token` 仅兼容
  - 非浏览器客户端：`Authorization: Bearer <token>`
- 消息格式：JSON Envelope，服务端推送 `type=event`，payload 内含 `event/id/data`
- 应用层握手：连接建立后服务端发送 `ready`（含 `protocol`/`policy`）；客户端建议先发送 `connect` 携带协议版本与客户端信息，不兼容会返回 `error` 并关闭连接；未发送 `connect` 时按默认协议版本处理
- 慢客户端告警：当客户端消费过慢导致队列压力时，服务端会发送 `event=slow_client`，前端可提示用户触发 `resume`
- 多路复用：同一连接可并发多个请求，需设置 `request_id`；服务端 `event/error` 会回传对应 `request_id`
- 断线续传：客户端发送 `resume` + `after_event_id`，服务端从 `stream_events` 回放并继续推送
- 实时订阅：客户端发送 `watch` + `after_event_id`，服务端持续推送会话流事件（直到取消或断线）
- 详细协议与节点说明：见 `docs/WebSocket-Transport.md`

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

## 5. 附录：辅助脚本

- `scripts/update_feature_log.py`：按分类写入 `docs/功能迭代.md`（支持 `--type/--scope`），默认使用 UTF-8 BOM 避免乱码。
