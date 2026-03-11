# wunder API 文档

## 4. API 设计

### 4.0 实现说明

- 接口实现基于 Rust Axum，路由拆分在 `src/api`（core/chat/user_world/user_tools/user_agents/user_channels/admin/a2a/desktop 等模块）。
- 当前产品核心能力采用“五维能力框架”：**形态协同 / 租户治理 / 智能体协作 / 工具生态 / 接口开放**；用户体系聊天（用户↔智能体 + 用户↔用户）是默认主线。
- 运行与热重载环境建议使用 `Dockerfile` + `docker-compose-x86.yml`/`docker-compose-arm.yml`。
- MCP 服务容器：`extra-mcp` 用于运行 `extra_mcp/` 下的 FastMCP 服务脚本，默认以 streamable-http 暴露端口，人员数据库连接通过 `extra_mcp/mcp_config.json` 的 `database` 配置。
- MCP 配置文件：`extra_mcp/mcp_config.json` 支持集中管理人员数据库配置，可通过 `MCP_CONFIG_PATH` 指定路径，数据库配置以配置文件为准。
- 多数据库支持：在 `mcp_config.json` 的 `database.targets` 中配置多个数据库（MySQL/PostgreSQL），默认使用 `default_key`，需要切换目标可调整 `default_key` 或部署多个 MCP 实例。
- Database query tools: configure `database.tables` (or `database.query_tables`) to auto-register table-scoped `db_query` tools (`db_query` for single table, `db_query_<key>` for multiple). Each tool is hard-bound to its table and embeds compact schema hints (`column + type`) in description.
- 单库类型切换：设置 `database.db_type=mysql|postgres`，或在多库配置中为每个目标指定 `type/engine` 或 DSN scheme。
- 知识库 MCP：按 `knowledge.targets` 动态注册 `kb_query` 工具（单目标为 `kb_query`，多目标自动命名为 `kb_query_<key>`）；向量知识库检索不依赖 RAGFlow MCP。
- 向量知识库使用 Weaviate，连接参数位于 `vector_store.weaviate`（url/api_key/timeout_s/batch_size）。
- docker compose 默认使用两个命名卷：`wunder_workspaces` 挂载到 `/workspaces`（用户工作区）；`wunder_logs` 挂载到 PostgreSQL/Weaviate 数据目录（`/var/lib/postgresql/data`、`/var/lib/weaviate`）；`/wunder/temp_dir/*` 默认落在本地 `./temp_dir`（容器内 `/app/temp_dir`，可用 `WUNDER_TEMP_DIR_ROOT` 覆盖）；运行态可写配置保留在仓库本地 `data/`（`data/config`、`data/prompt_templates`、`data/user_tools` 等）。构建/依赖缓存（`target/`、`.cargo/`、`frontend/node_modules/`）保持写入仓库目录便于管理。
- 沙盒服务：独立容器运行 `wunder-server` 的 `sandbox` 模式（`WUNDER_SERVER_MODE=sandbox`），对外提供 `/sandboxes/execute_tool` 与 `/sandboxes/release`，由 `WUNDER_SANDBOX_ENDPOINT` 指定地址。
- 工具清单与提示词注入复用统一的工具规格构建逻辑：`tool_call/freeform_call` 模式会注入工具协议片段，`function_call` 模式不注入工具提示词，工具清单仅用于 tools 协议。
- 当 `tool_call_mode=freeform_call` 且模型走 OpenAI Responses API 时，服务端会把 `apply_patch` 这类语法工具下发为原生 `type=custom` 工具（携带 `format={type:grammar,syntax:lark,definition}`），普通 JSON 工具继续走 `type=function`；工具结果会按 `custom_tool_call_output/function_call_output` 回填历史，避免仅靠 XML 提示词驱动。
- 配置分层：基础配置优先读取 `config/wunder.yaml`（`WUNDER_CONFIG_PATH` 可覆盖）；若不存在则自动回退 `config/wunder-example.yaml`；管理端修改会写入 `data/config/wunder.override.yaml`（`WUNDER_CONFIG_OVERRIDE_PATH` 可覆盖）。
- 环境变量：`.env` 为可选项；docker compose 通过 `${VAR:-default}` 提供默认值，未提供 `.env` 也可直接启动。
- compose 镜像策略：`docker-compose-x86.yml` / `docker-compose-arm.yml` 的 `wunder-server` / `wunder-sandbox` / `extra-mcp` 统一使用同名本地镜像（`wunder-x86`/`wunder-arm`），并设置 `pull_policy: never`，已存在镜像时优先复用，不存在时再自动构建，避免首次启动时 `extra-mcp` 先拉取失败。
- 前端入口：管理端调试 UI `http://127.0.0.1:18000`，调试前端 `http://127.0.0.1:18001`（Vite dev server），用户侧前端 `http://127.0.0.1:18002`（Nginx 静态服务）。
- Single-port docker compose mode: expose only `18001` publicly; proxy `/wunder`, `/a2a`, and `/.well-known/agent-card.json` to `wunder-server:18000`; keep `wunder-postgres`/`wunder-weaviate`/`extra-mcp` bound to `127.0.0.1`.
- 鉴权：管理员接口使用 `X-API-Key` 或 `Authorization: Bearer <api_key>`（配置项 `security.api_key`），用户侧接口使用 `/wunder/auth` 颁发的 `Authorization: Bearer <user_token>`；外部系统嵌入接入使用 `security.external_auth_key`（环境变量 `WUNDER_EXTERNAL_AUTH_KEY`）调用 `/wunder/auth/external/*`。
- 默认管理员账号为 admin/admin，服务启动时自动创建且不可删除，可通过用户管理重置密码。
- 用户端请求可省略 `user_id`，后端从 Token 解析；管理员接口可显式传 `user_id` 以指定目标用户。
- 模型配置新增 `model_type=llm|embedding`，向量知识库依赖 embedding 模型调用 `/v1/embeddings`。
- 用户侧前端默认入口为 `/app/home`（desktop 为 `/desktop/home`）；`/app/home|chat|user-world|workspace|tools|settings|profile|channels|cron` 统一复用 Messenger 壳。外链详情路由为 `/app/external/:linkId`（demo 为 `/demo/external/:linkId`）。External links are managed via `/wunder/admin/external_links` and delivered by `/wunder/external_links` after org-level filtering; production frontend port is 18002, development port is 18001。
- 当使用 API Key/管理员 Token 访问 `/wunder`、`/wunder/chat`、`/wunder/workspace`、`/wunder/user_tools` 时，`user_id` 允许为“虚拟用户”，无需在 `user_accounts` 注册，仅用于线程/工作区/工具隔离。
- 工作区容器约定：用户私有容器固定为 `container_id=0`，智能体容器范围为 `1~10`；`/wunder/workspace*` 全部接口（含 upload）支持显式 `container_id`，且优先级高于 `agent_id` 推导。
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

### 4.0.2 用户世界（User World）接口

- 目标：支持“用户↔用户”单聊 + 群聊，默认可见联系人，WebSocket 优先，SSE 兜底。
- 鉴权：使用用户端 Bearer Token（与 `/wunder/chat/*` 一致）。
- 接口清单：
  - `GET /wunder/user_world/contacts`：联系人列表（支持 `keyword/offset/limit`，返回 `online/last_seen_at` 在线状态）
  - `GET /wunder/user_world/groups`：当前用户群聊列表（支持 `offset/limit`）
  - `POST /wunder/user_world/groups`：创建群聊（`group_name/member_user_ids[]`）
  - `GET /wunder/user_world/groups/{group_id}`：群详情（含群公告与成员列表，需群成员权限）
  - `POST /wunder/user_world/groups/{group_id}/announcement`：更新群公告（`announcement`，传空可清空，需群成员权限）
  - `POST /wunder/user_world/conversations`：按 `peer_user_id` 获取或创建 direct 会话
  - `GET /wunder/user_world/conversations`：当前用户会话列表
  - `GET /wunder/user_world/conversations/{conversation_id}`：会话详情（需成员权限）
  - `GET /wunder/user_world/conversations/{conversation_id}/messages`：消息分页（`before_message_id/limit`）
  - `POST /wunder/user_world/conversations/{conversation_id}/messages`：发送消息（`content/content_type/client_msg_id`）
  - `POST /wunder/user_world/conversations/{conversation_id}/read`：回写已读（`last_read_message_id`）
- `GET /wunder/user_world/files/download`：会话内文件/文件夹下载（`conversation_id/owner_user_id/path`，可选 `container_id` 指定容器；目录会自动打包为 zip，支持 `check=true` 仅校验存在并返回响应头）
  - `GET /wunder/user_world/conversations/{conversation_id}/events`：SSE 事件流（`after_event_id/limit`）
  - `GET /wunder/user_world/ws`：WebSocket 多路复用通道
- WS 消息类型：
  - client：`connect` / `watch` / `send` / `read` / `cancel` / `ping`
  - server：`ready` / `event` / `error` / `pong`
- 事件类型：
  - `uw.message`：新消息事件
  - `uw.read`：读状态更新事件
- 协议约束：
  - 发送者身份以 Token 用户为准，不允许伪造。
  - 仅会话成员可读写与订阅事件。
  - 支持 `client_msg_id` 幂等去重（同会话内唯一）。
  - 语音消息约定：当 `content_type=voice`（或 `audio/*`）时，`content` 推荐传 JSON 字符串，至少包含 `path`（容器相对路径）；可选字段 `duration_ms/mime_type/name/size/container_id/owner_user_id`。
  - 会话对象在群聊场景返回 `group_id/group_name/member_count`；单聊场景返回 `peer_user_id`。
  - 群聊对象返回 `announcement/announcement_updated_at` 字段；群详情额外返回 `members[]`。

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
- `attachments`：数组，可选，附件列表（图片/音频支持 data URL；服务端会持久化到用户私有容器并补充 `public_path`）
- 约束：注册用户每日有请求额度，按每次模型调用消耗，超额返回 429（`detail.code=USER_QUOTA_EXCEEDED`）。
- 约束：`question` 与非图片附件文本合计最多 `1048576` 个字符，超出返回 400（`detail.field=input_text`，并携带 `detail.max_chars/detail.actual_chars`）。
- 忙时队列：当 `agent_queue.enabled=true` 时，非流式返回 202（`data.queue_id`/`data.thread_id`/`data.session_id`），SSE/WS 返回 `queued` 事件。
- 忙时返回：当 `agent_queue.enabled=false` 且显式指定 `session_id` 正在运行/取消中时，会返回 429（`detail.code=USER_BUSY`）。
- 说明：未传 `session_id` 且主会话正忙时，会自动分叉独立会话继续处理，并返回新的 `session_id`（不覆盖主会话）。
- 说明：问询面板进入 `waiting` 后，用户选择路线会被当作正常请求立即继续处理，不会被判定为“会话繁忙”进入队列。
- 约束：全局并发上限由 `server.max_active_sessions` 控制，超过上限的请求会排队等待。
- 约束：同一轮同类工具连续失败达到 `server.tool_failure_guard_threshold`（默认 5）会触发 `tool_failure_guard` 并停止自动重试。
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
- `搜索内容`（`search_content`）新增 `query_mode=literal|regex`、`case_sensitive`、`context_before/context_after` 入参；返回保留兼容字段 `matches`，并新增结构化 `hits`（命中行 + 前后文 + 高亮分段）以便前端直接渲染。
- 新增内置工具 `计划面板`（英文别名 `update_plan`），用于更新计划看板并触发 `plan_update` 事件。
- 新增内置工具 `问询面板`（英文别名 `question_panel`/`ask_panel`），用于提供多条路线选择并触发 `question_panel` 事件。
- 新增内置工具 `技能调用`（英文别名 `skill_call`/`skill_get`），传入技能名返回完整 SKILL.md 与技能目录结构。
  - 技能文档内建议使用占位符 `{{SKILL_ROOT}}` 引用技能资源（脚本/示例/工作流文件等）。
  - `skill_call` 返回时会将 `skill_md` 中的 `{{SKILL_ROOT}}` 自动替换为本次可见的技能根目录绝对路径（同返回字段 `root`）。
- 新增内置工具 `子智能体控制`（英文别名 `subagent_control`），通过 `action=list|history|send|spawn` 统一完成会话列表/历史/发送/派生。
- 新增内置工具 `智能体蜂群`（英文别名 `agent_swarm`/`swarm_control`），通过 `action=list|status|send|history|spawn|batch_send|wait` 管理当前用户“当前智能体以外”的其他智能体。
- `智能体蜂群` 的 `send` 支持按 `agent_id` 自动复用会话；无主会话时会自动创建后再发送指令。
- `智能体蜂群` 新增 `wait` 动作：可直接等待 `run_ids` 结果并返回聚合状态，避免母蜂反复轮询 `status`。
- 多工蜂协作推荐：先 `batch_send` 一次并发派发，再 `wait` 统一收敛。
- `智能体蜂群` 入参语义增强（便于模型主动调用）：`spawn` 需 `agentId+task`，`send` 需 `message` 且 `agentId/sessionKey` 二选一，`history` 需 `sessionKey`，`wait` 需 `runIds`，`batch_send` 需 `tasks[]`（每项需 `message` 且 `agentId/sessionKey` 二选一）。
- 推荐最短调用路径：`list -> batch_send -> wait -> history/status`（单目标用 `send` 替代 `batch_send`）。
- `子智能体控制` 的 `send` 支持 `timeoutSeconds` 等待回复，`spawn` 支持 `runTimeoutSeconds` 等待完成并返回 `reply/elapsed_s`。
- 新增内置工具 `节点调用`（英文别名 `node.invoke`/`node_invoke`），通过 `action=list|invoke` 统一完成节点发现与节点调用。
- 新增内置工具 `用户世界工具`（英文别名 `user_world`），通过 `action=list_users|send_message` 获取用户列表或发送私信（消息会在用户世界页面可见）。
- 新增内置工具 `浏览器`（英文别名 `browser`），通过 `action=navigate|click|type|screenshot|read_page|close` 统一操作，仅 desktop 模式可用。
- 新增内置工具 `桌面控制器`（英文别名 `desktop_controller`/`controller`），通过 bbox+action 执行桌面操作，执行后自动附加桌面截图，仅 desktop 模式可用。
- 新增内置工具 `桌面监视器`（英文别名 `desktop_monitor`/`monitor`），等待 wait_ms 后返回桌面截图并自动附加，仅 desktop 模式可用。
- 新增内置工具 `休眠等待`（英文别名 `sleep`/`sleep_wait`/`pause`），参数 `seconds` 必填；用于主动等待（如 `300` 秒），并自动适配工具超时。
- 新增内置工具 `读图工具`（英文别名 `read_image`/`view_image`），参数 `path` 必填、`prompt` 可选；执行成功后会在下一轮自动附加 `image_url` 供模型视觉分析。
- `读图工具` 仅在 `llm.models.<name>.support_vision=true` 的模型下会出现在可用工具列表中，非视觉模型会自动隐藏并拒绝调用。
- `桌面控制器/桌面监视器` 仅在 `llm.models.<name>.support_vision=true` 的模型下会出现在可用工具列表中。
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
  - `skills`：技能列表（name/description/path/input_schema/enabled/shared/builtin/source/readonly）
    - `source`：`builtin` 或 `custom`
    - `builtin=true`/`readonly=true` 表示内置技能（只读）
- `POST` 入参（JSON）：
  - `user_id`：用户唯一标识
  - `enabled`：启用技能名列表
  - `shared`：共享技能名列表
- `POST` 返回：同 `GET`
- 说明：desktop 本地模式下，内置技能启用状态会同步写入全局 `skills.enabled`，不再作为 `user_id@技能名` 自建工具注入。
- `DELETE` 入参（Query）：
  - `user_id`：用户唯一标识
  - `name`：技能名称
- `DELETE` 返回（JSON）：
  - `ok`：是否成功
  - `name`：技能名称
  - `message`：提示信息
- 说明：内置技能只读，`DELETE` 会返回 `403`。

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
- 说明：内置技能只读，`PUT` 会返回 `403`。

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
- 说明：上传内容写入自定义技能目录（`source=custom`），不会覆盖内置 `skills/` 源码目录。
- 说明：上传目录若与内置技能目录冲突会返回 `403`（避免覆盖内置技能）。

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
- 说明：desktop 本地模式下，该接口的“管理员开放工具”仅返回 `builtin_tools`，`mcp_tools/a2a_tools/skills/knowledge_tools` 统一置空；其余工具通过用户自建与共享入口管理。

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
- 说明：默认从项目根目录 `temp_dir/` 目录读取文件并下载；可通过环境变量 `WUNDER_TEMP_DIR_ROOT` 指定根目录。
- 返回：文件流（`Content-Disposition: attachment`）

### 4.1.2.25 `/wunder/temp_dir/upload`

- 方法：`POST`
- 鉴权：无
- 类型：`multipart/form-data`
- 入参：
  - `file` 文件字段（支持多个同名字段）
  - `path` 目标子目录路径（相对 `temp_dir/`，可选）
  - `overwrite` 是否覆盖同名文件（可选，默认 true）
- 说明：默认上传文件到项目根目录 `temp_dir/`，若设置 `path` 则自动创建目录；可通过环境变量 `WUNDER_TEMP_DIR_ROOT` 指定根目录。
- 返回（JSON）：
  - `ok`：是否成功
  - `files`：上传后的文件名列表

### 4.1.2.26 `/wunder/temp_dir/list`

- 方法：`GET`
- 鉴权：无
- 说明：列出临时目录文件（包含子目录，返回相对路径）；默认根目录为项目根 `temp_dir/`，可通过环境变量 `WUNDER_TEMP_DIR_ROOT` 指定。
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
- 说明：默认操作项目根目录 `temp_dir/`；可通过环境变量 `WUNDER_TEMP_DIR_ROOT` 指定根目录。
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
  - 返回：`data.jobs`（包含 job_id/name/schedule/next_run_at/last_status/consecutive_failures/auto_disabled_reason 等）
- `GET /wunder/cron/status`：查询调度器健康状态与当前用户任务概况
  - 返回：`data.scheduler`（started/enabled/running_jobs/next_run_at/last_tick_at/last_error、`poll_interval_ms`、`max_idle_sleep_ms`、`lease_ttl_ms`、`lease_heartbeat_ms`、`max_concurrent_runs`、`idle_retry_ms`、`max_busy_wait_ms`、`max_consecutive_failures` 等）+ `data.jobs_total/jobs_enabled/jobs_running`；任务项额外返回派生字段 `running/heartbeat_at/lease_expires_at` 用于前端与模型判断当前执行态。
- `GET /wunder/cron/runs?job_id=...&limit=...`：查询任务运行记录
  - 返回：`data.runs`
- `POST /wunder/cron/add|update|remove|enable|disable|get|run|action`：新增/更新/删除/启停/查询/立即执行（`action=status` 与 `GET /wunder/cron/status` 等价）
  - 入参：与内置工具 `schedule_task` schema 一致（`action` + `job`）
  - 说明：
    - `job.schedule.kind=every` 时支持可选 `schedule.at` 作为首次触发时间锚点；若未提供则默认以任务创建时间为起点，首次触发为“下一个间隔点”（严格晚于当前时刻，避免创建即触发）。
    - 可选 `job.schedule_text` 支持自然语言或 cron（如 `every 5 minutes`、`daily at 9am`、`0 */6 * * *`）；若同时传 `schedule` 与 `schedule_text`，以 `schedule` 为准。
    - `schedule.at` 必须是未来时间且不超过 1 年；`schedule.every_ms` 最大 24 小时；`schedule.cron` 需为 5-7 段字段。
    - 调度执行遇到 `USER_BUSY` 会按 `cron.idle_retry_ms` 重试，并受 `cron.max_busy_wait_ms` 上限保护，超时后写入 error 运行记录。
    - 连续失败达到 `cron.max_consecutive_failures` 会自动停用任务并写入 `auto_disabled_reason`。
    - 周期任务失败后会按退避策略推迟下一次执行时间，取“自然下一次执行时间”和“错误退避时间”中的较大值，降低高错误率场景下的重试风暴。
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
  - `skills`：技能信息（name/description/path/input_schema/enabled/builtin/source/readonly/editable）
    - `source`：`builtin` / `custom` / `external`
- `POST` 入参：
  - `enabled`：启用技能名列表
  - `paths`：技能目录列表（可选）
- `DELETE` 入参（Query）：
  - `name`：技能名称
- `DELETE` 返回：
  - `ok`：是否删除成功
  - `name`：已删除技能名称
  - `message`：删除说明
- 说明：仅允许删除自定义上传技能（`source=custom`）；内置技能只读会返回 `403`。

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
- 说明：仅 `source=custom` 技能允许写入；内置/外部技能只读会返回 `403`。

### 4.1.6 `/wunder/admin/llm`

- 方法：`GET/POST`
- `GET` 返回：
  - `llm.default`：默认模型配置名称
- `llm.models`：模型配置映射（model_type/provider/api_mode/base_url/api_key/model/temperature/timeout_s/retry/max_rounds/max_context/max_output/support_vision/support_hearing/stream/stream_include_usage/tool_call_mode/history_compaction_ratio/history_compaction_reset/stop/enable/mock_if_unconfigured）
  - 说明：`retry` 同时用于请求失败重试与流式断线重连。
  - 说明：`provider` 支持 OpenAI 兼容预置（`openai_compatible/openai/openrouter/siliconflow/deepseek/moonshot/qwen/groq/mistral/together/ollama/lmstudio`），除 `openai_compatible` 外其余可省略 `base_url` 自动补齐。
  - 说明：`model_type=embedding` 表示嵌入模型，向量知识库会使用其 `/v1/embeddings` 能力。
  - 说明：`api_mode` 可选 `chat_completions|responses`（默认 chat_completions；当 provider=openai 且模型为 GPT-5/O 系列时未配置会自动走 responses），`responses` 会改用 `/v1/responses` 协议与流式事件。
  - 说明：`max_rounds` 缺省为 1000；非管理员会话在未配置或过低时会提升到至少 2（含工具调用），管理员与 desktop 模式不受该限制。
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

### 4.1.6.5 `/wunder/admin/prompt_templates`

- 方法：`GET`
- 返回（JSON）：
  - `data.active`：当前启用的系统提示词模板包 ID（`default` 表示仓库内 `prompts/`）
  - `data.packs_root`：非 default 模板包的根目录（默认 `./data/prompt_templates`）
  - `data.packs[]`：模板包列表（id/is_default/path）
  - `data.segments[]`：系统提示词分段文件列表（key/file）

### 4.1.6.6 `/wunder/admin/prompt_templates/active`

- 方法：`POST`
- 入参（JSON）：
  - `active`：要启用的模板包 ID（空或 `default` 表示仓库内默认模板）
- 返回（JSON）：
  - `ok`：是否成功
  - `data.active`：更新后的启用模板包 ID

### 4.1.6.7 `/wunder/admin/prompt_templates/file`

- 方法：`GET/PUT`
- `GET` 入参（Query）：
  - `pack_id`：模板包 ID（可选，默认使用当前启用包）
  - `locale`：`zh`/`en`（可选，默认跟随系统语言设置）
  - `key`：分段 key（role/engineering/tools_protocol/skills_protocol/memory/extra）
- `GET` 返回（JSON）：
  - `data.pack_id`：模板包 ID
  - `data.locale`：`zh`/`en`
  - `data.key`：分段 key
  - `data.path`：文件路径（服务端解析后的实际路径）
  - `data.exists`：文件是否存在于该模板包
  - `data.fallback_used`：是否回退读取 default 模板包内容
  - `data.content`：文件内容
- `PUT` 入参（JSON）：
  - `pack_id`：模板包 ID（可选，默认使用当前启用包）
  - `locale`：`zh`/`en`（可选，默认跟随系统语言设置）
  - `key`：分段 key
  - `content`：文件内容
- `PUT` 返回（JSON）：
  - `ok`：是否成功
  - `data.path`：写入文件路径
- 说明：
  - `default` 模板包为只读，禁止通过 `PUT` 修改；请先创建新模板包再编辑。

### 4.1.6.8 `/wunder/admin/prompt_templates/packs`

- 方法：`POST`
- 入参（JSON）：
  - `pack_id`：要创建的模板包 ID（仅支持字母/数字/_/-）
  - `copy_from`：可选，复制来源模板包 ID（默认 `default`）
- 返回（JSON）：
  - `ok`：是否成功
  - `data.pack_id`：模板包 ID
  - `data.path`：模板包路径
  - `data.copied_from`：复制来源模板包 ID

### 4.1.6.9 `/wunder/admin/prompt_templates/packs/{pack_id}`

- 方法：`DELETE`
- 返回（JSON）：
  - `ok`：是否成功
  - `data.pack_id`：删除的模板包 ID

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
- `session.context_tokens/context_tokens_peak` 汇总优先采用 `token_usage.input_tokens`（模型实际接收上下文）作为有效占用；`context_usage` 仍保留估算值用于过程观测。


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
- 说明：已登录用户可显式传入自身 scoped `user_id`（如 `user__c__2`、`user__a__xxxx`、`user__agent__legacy`）访问对应容器/智能体工作区，无需管理员权限。
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
- 用户管理：`/wunder/admin/user_accounts`、`/wunder/admin/user_accounts/test/seed`、`/wunder/admin/user_accounts/test/cleanup`、`/wunder/admin/user_accounts/{user_id}`、`/wunder/admin/user_accounts/{user_id}/password`、`/wunder/admin/user_accounts/{user_id}/tool_access`。
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

### 4.1.35 记忆管理接口（已移除）

- 原 `/wunder/admin/memory/*` 管理端接口已下线，不再提供管理员侧记忆面板能力。
- 当前推荐方式：通过内置工具 `记忆管理`（`memory_manager`）由智能体按需维护自己的长期记忆条目。
- 作用域：按 `用户 + 智能体` 隔离；记忆变更仅影响新会话，已有会话保持原提示词快照。

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
  - `file_ops`：列出文件/写入/读取/搜索/应用补丁组合耗时。
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
  - desktop 本地模式下，若传入 `unit_id`，允许直接绑定任意非空字符串（不校验组织层级）。
  - 返回（JSON）：`data.access_token`、`data.user`（UserProfile）
- `POST /wunder/auth/login`
  - 入参（JSON）：`username`、`password`
  - 返回：同注册
- `POST /wunder/auth/demo`
  - 入参（JSON）：`demo_id`（可选）
  - 返回：同注册
- `POST /wunder/auth/external/login`
  - 用途：外部系统用户直连 wunder，账号不存在时自动创建，存在时自动对齐密码并登录。
  - 配置：需启用 `security.external_auth_key`（或环境变量 `WUNDER_EXTERNAL_AUTH_KEY`）。
  - 入参（JSON）：`key`、`username`、`password`、`unit_id`（可选）
  - desktop 本地模式下，`unit_id` 支持直接写入任意非空字符串（不依赖组织树）。
  - 返回（JSON）：`data.access_token`、`data.user`（UserProfile）、`data.created`（是否新建）、`data.updated`（是否更新既有账号信息）
- `POST /wunder/auth/external/code`
  - 用途：为 iframe 嵌入场景签发一次性登录码（推荐由外部系统后端调用）。
  - 入参（JSON）：同 `/wunder/auth/external/login`
  - 返回（JSON）：`data.code`、`data.expires_at`、`data.created`、`data.updated`
- `POST /wunder/auth/external/exchange`
  - 用途：前端用一次性登录码换取用户 Token（单次有效，默认 60 秒过期）。
  - 入参（JSON）：`code`
  - 返回（JSON）：`data.access_token`、`data.user`（UserProfile）
- `GET /wunder/auth/org_units`
  - 入参：无
  - 返回（JSON）：`data.items`（单位列表）、`data.tree`（单位树）
  - desktop 本地模式下，单位按扁平结构返回（`parent_id=null`、`level=1`），并自动合并用户资料中出现过的 `unit_id`，用于“我的概况”直接录入单位名称后快速归拢联系人。
- `GET /wunder/auth/me`
  - 鉴权：Bearer Token
  - 返回（JSON）：`data`（UserProfile）
- `PATCH /wunder/auth/me`
  - 鉴权：Bearer Token
  - 入参（JSON）：`username`（可选）、`email`（可选）、`unit_id`（可选）
  - desktop 本地模式下，`unit_id` 允许直接填写任意非空字符串（留空表示清除），不再校验组织树层级。
  - 返回（JSON）：`data`（UserProfile）
- `GET /wunder/auth/me/preferences`
  - 鉴权：Bearer Token
  - 用途：获取当前用户的跨端外观偏好（主题/头像）。
  - 返回（JSON）：`data.theme_mode`、`data.theme_palette`、`data.avatar_icon`、`data.avatar_color`、`data.updated_at`（秒级时间戳，0 表示未落库）
- `PATCH /wunder/auth/me/preferences`
  - 鉴权：Bearer Token
  - 入参（JSON，可部分提交）：`theme_mode`（dark/light）、`theme_palette`（hula-green/eva-orange/minimal）、`avatar_icon`（initial 或 qq-avatar-xxxx）、`avatar_color`（#RRGGBB）
  - 返回（JSON）：同 `GET /wunder/auth/me/preferences`；后端会做字段规范化与容错。
- iframe 嵌入辅助：
  - 用户侧前端支持在 URL Query 中携带 `wunder_token`（或 `access_token`）自动登录，并在首跳时自动移除敏感参数。
  - 用户侧前端支持在 URL Query 中携带 `wunder_code`，启动后自动调用 `/wunder/auth/external/exchange` 换取 Token 并完成登录。
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
- 适配器处理顺序（注册表驱动）：
  1. 服务端会先将 `provider` 统一转为小写并查找 `ChannelAdapterRegistry`。
  2. 若命中适配器，先执行 `verify_inbound`（失败返回 `401`），再执行 `parse_inbound`（失败返回 `400`）。
  3. 若适配器 `parse_inbound` 返回 `None` 或未命中适配器，则回退到通用 `ChannelMessage` JSON 解析。
- 渠道审批交互：当工具命中审批策略时，系统会向渠道回发审批提示文本；用户可在原会话回复 `1/2/3`（分别对应同意一次/同意本会话/拒绝）继续流程，`/stop` 会同时取消当前会话与待审批请求。

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


### 4.1.54.4 `/wunder/channel/wechat/webhook`（企业微信）

- 方法：`GET/POST`
- 鉴权：
  - `GET` URL 验证使用 `msg_signature/timestamp/nonce/echostr` + `wechat.token` 校验签名。
  - `POST` 回调统一要求签名参数 + `timestamp` + `nonce` + `wechat.token`；支持 `msg_signature/msgsignature/signature` 参数名。
  - `POST` 明文模式（无 `<Encrypt>`）校验回调签名；加密模式（有 `<Encrypt>`）校验消息签名并用 `wechat.encoding_aes_key` 执行 AES-CBC 解密。
- Query：
  - `account_id`：可选，优先指定渠道账号。
  - `msg_signature` / `msgsignature` / `signature`、`timestamp`、`nonce`：企业微信回调签名参数（兼容不同模式参数名）。
  - `echostr`：仅 `GET` URL 验证时使用。
- Header：
  - `x-channel-account`：可选，覆盖 `account_id`。
- URL 验证（GET）：
  - 传入 `msg_signature/timestamp/nonce/echostr` 后返回解密后的明文字符串。
- 消息回调（POST）：
  - 入参为企业微信 XML（支持明文或 `<Encrypt>` 加密包）。
  - 当前支持 `MsgType=text/voice/image/file/location`；其中 `voice` 优先取 `Recognition`，`event` 仅处理可映射文本的场景（如菜单点击）。
  - `enter_agent/subscribe/unsubscribe` 等事件会忽略并直接返回 `success`。
  - 当签名校验失败时返回 `401 wechat signature mismatch`。
  - 成功返回纯文本 `success`（非 JSON），避免企业微信重复回调。
  - 文本消息采用异步处理：服务端先快速回 `success`，再在后台完成模型调用与出站回包，规避渠道侧 5 秒超时重试。
  - 多账号场景下会优先按签名匹配账号，若命中多个候选则尝试按解密后的 `AgentID` 精确归属。
- 出站投递：
  - 当 `ChannelAccount.config.wechat.corp_id/agent_id/secret` 可用时，调用企业微信 API 直连发送文本：
    - 先请求 `/cgi-bin/gettoken`
    - 再请求 `/cgi-bin/message/send`
  - 长文本会按企业微信上限（2048 字节）自动分片发送。
  - 服务端会缓存企业微信 `access_token`（提前刷新），降低 token 接口压力和发送延迟。
  - `peer.kind=user` -> `touser`
  - `peer.kind=group` -> `toparty`
  - `peer.kind=tag` -> `totag`
- 配置（ChannelAccount.config）：
  - `wechat.corp_id`
  - `wechat.agent_id`
  - `wechat.secret`
  - `wechat.token`（回调签名校验）
  - `wechat.encoding_aes_key`（回调解密）
  - `wechat.domain`（可选，默认 `qyapi.weixin.qq.com`）

### 4.1.54.5 `/wunder/channel/wechat_mp/webhook`（微信公众号）

- 方法：`GET/POST`
- 鉴权：
  - 明文模式使用 `signature/timestamp/nonce` + `wechat_mp.token` 校验签名。
  - 安全模式使用 `msg_signature/timestamp/nonce` + `wechat_mp.token` 校验消息签名。
  - 安全模式下需配置 `wechat_mp.encoding_aes_key` 解密 `<Encrypt>` 载荷。
- Query：
  - `account_id`：可选，优先指定渠道账号。
  - `msg_signature` 或 `signature`、`timestamp`、`nonce`：公众号回调签名参数（兼容不同模式参数名）。
  - `echostr`：仅 `GET` URL 验证时使用。
  - `encrypt_type`：`POST` 可选（如 `aes`）。
- Header：
  - `x-channel-account`：可选，覆盖 `account_id`。
- URL 验证（GET）：
  - 签名通过后返回明文 challenge；若配置了 `encoding_aes_key`，会优先尝试解密 `echostr`。
- 消息回调（POST）：
  - 入参为公众号 XML（支持明文或 `<Encrypt>` 加密包）。
  - 当前支持 `MsgType=text`、`MsgType=voice`（优先取 `Recognition`），`event` 仅映射可转文本的事件；`unsubscribe` 事件会忽略。
  - 成功返回纯文本 `success`（非 JSON）。
  - 文本消息采用异步处理：先快速回 `success`，后台完成模型调用与出站回包，规避 5 秒超时重试。
- 账号识别：
  - 支持 `account_id` 直指账号。
  - 未显式指定时，优先用签名匹配 `wechat_mp.token`，其次用 `ToUserName` 匹配 `wechat_mp.original_id`/`wechat_mp.app_id`。
- 出站投递：
  - 当 `ChannelAccount.config.wechat_mp.app_id/app_secret` 可用时，调用公众号 API 直连发送文本：
    - 先请求 `/cgi-bin/token`
    - 再请求 `/cgi-bin/message/custom/send`
  - 服务端会缓存公众号 `access_token`（提前刷新）。
  - 当前默认按 `peer.kind=user` 发送到 `touser`（OpenID）。
- 配置（ChannelAccount.config）：
  - `wechat_mp.app_id`
  - `wechat_mp.app_secret`
  - `wechat_mp.token`（回调签名校验，可选但建议配置）
  - `wechat_mp.encoding_aes_key`（回调解密，可选）
  - `wechat_mp.original_id`（用于账号识别，可选）
  - `wechat_mp.domain`（可选，默认 `api.weixin.qq.com`）

### 4.1.54.6 `/wunder/channel/qqbot/webhook`（QQ Bot）

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

### 4.1.54.7 `/wunder/channel/xmpp/webhook`（XMPP 标准客户端）

- 方法：`POST`
- 说明：该接口与通用 webhook 一致，主要用于兼容手工调试/回放；XMPP 生产入站默认由内置长连接 worker 接收并投递到 ChannelHub。
- 账号配置（`ChannelAccount.config.xmpp`）：
  - `jid`：登录 JID（必填）
  - `password`：登录密码（可选；与 `password_env` 二选一）
  - `password_env`：登录密码环境变量名（可选；兼容 openfang `channels.xmpp.password_env`）
  - `domain`：SRV 域名或手动连接域名（可选）
  - `host`、`port`：手动连接地址（可选）
  - `server`：`host` 的兼容别名（可选；兼容 openfang `channels.xmpp.server`）
  - `direct_tls`：是否使用 5223 默认端口（可选，默认 `false`）
  - `trust_self_signed`：是否信任自有/自签证书（可选，默认 `true`）
  - `resource`：登录资源（可选）
  - `muc_nick`：群聊昵称（可选，用于过滤自身群消息）
  - `muc_rooms`：自动加入房间列表（可选，数组或逗号分隔字符串）
  - `rooms`：`muc_rooms` 的兼容别名（可选；兼容 openfang `channels.xmpp.rooms`）
  - `long_connection_enabled`：是否启用长连接（可选，默认 `true`）
  - `send_initial_presence`：连接后是否发送初始 presence（可选，默认 `true`）
  - `status_text`：presence 状态文案（可选）
  - `heartbeat_enabled`：是否启用主动心跳（可选，默认 `true`）
  - `heartbeat_interval_s`：主动心跳间隔秒数（可选，默认 `60`）
  - `heartbeat_timeout_s`：主动心跳超时秒数（可选，默认 `20`）
  - `respond_ping`：是否自动应答对端 IQ ping（可选，默认 `true`）
- 运行机制：
  - 当账号 `status=active` 且 `xmpp.long_connection_enabled=true` 且凭证完整时，后台 worker 会自动建立 XMPP 长连接。
  - 长连接接收文本消息后会标准化为 `ChannelMessage`，并进入 `handle_inbound` 主链路。
  - 出站优先复用长连接会话发包；会话不可用时自动降级为短连接发送。
  - 心跳兼容：支持被动应答 IQ ping；启用主动心跳时会周期发送 ping，超时触发重连。

### 4.1.55 `/wunder/chat/*`

- `GET /wunder/chat/transport`：获取当前聊天流式通道策略
  - 返回：`data.chat_stream_channel`（`ws`/`sse`）

- `POST /wunder/chat/sessions`：创建会话
  - 入参（JSON）：`title`（可选）、`agent_id`（可选）
- 返回：`data`（id/title/created_at/updated_at/last_message_at/agent_id/tool_overrides/parent_session_id/parent_message_id/spawn_label/spawned_by/is_main）
- 说明：新会话默认不再固化智能体 `tool_names` 到 `tool_overrides`；空覆盖表示继承最新智能体默认与当前可用工具集合。
- `GET /wunder/chat/sessions`：会话列表
  - Query：`page`/`page_size` 或 `offset`/`limit`，可选 `agent_id`（空值表示通用聊天，省略表示不过滤），可选 `parent_session_id`（或 `parent_id`/`parentId`/`parentSessionId`）
- 返回：`data.total`、`data.items`（每项含 is_main 标记主线程）
- `GET /wunder/chat/sessions/{session_id}`：会话详情
  - Query：`limit`（消息条数，可选）
  - 返回：`data`（会话信息含 parent_session_id/parent_message_id/spawn_label/spawned_by + messages + history_has_more/history_before_id；进行中的会话会追加 stream_incomplete=true 的助手占位）
  - `messages[].attachments`：可选，附件数组，包含 `name/content/content_type/public_path`（若文件已删除前端可忽略渲染）
  - `messages[].history_id`：历史记录 id（用于历史分页）
  - `model_name`：当前默认模型名称（用于聊天页展示）
- `GET /wunder/chat/sessions/{session_id}/events`：会话事件（工作流还原）
  - 返回：`data.id`、`data.rounds`（user_round/events；事件内包含 `user_round`/`model_round`）、`data.running`、`data.last_event_id`
- `GET /wunder/chat/sessions/{session_id}/history`：分页加载会话历史
  - Query：`before_id`（可选，取历史记录 id，小于该值）、`limit`（可选，1~200，默认 80）
  - 返回：`data.id`、`data.messages`、`data.history_has_more`、`data.history_before_id`
  - `messages[].history_id`：历史记录 id（用于继续分页）
- `DELETE /wunder/chat/sessions/{session_id}`：删除会话
  - 会同时删除该会话关联的定时任务
  - 返回：`data.id`
- `POST /wunder/chat/sessions/{session_id}/messages`：发送消息（支持 SSE）
  - 入参（JSON）：`content`、`stream`（默认 true）、`attachments`（可选）、`tool_call_mode`（可选）、`approval_mode`（可选）
  - `attachments` 项：`{ name?, content?, mime_type?, public_path? }`（图片/音频可用 data URL；服务端落盘后返回 `public_path`）
  - 约束：`content` 与非图片附件文本合计最多 `1048576` 个字符，超出返回 400（`detail.field=input_text`，并携带 `detail.max_chars/detail.actual_chars`）
  - `approval_mode` 兼容别名：`approvalMode`、`approval_mode`、`permissionLevel`、`permission_level`
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
  - desktop 本地模式说明：技能挂载遵循用户技能总开关（`/wunder/user_tools/skills`），会话/智能体 `tool_overrides` 仅过滤非技能工具；`__no_tools__` 仍可禁用全部工具。
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
  - 返回：`data.total`、`data.items`（id/name/description/system_prompt/tool_names/access_level/approval_mode/is_shared/status/icon/sandbox_container_id/created_at/updated_at）
- `GET /wunder/agents/shared`：共享智能体列表
  - 返回：`data.total`、`data.items`（同上）
- `GET /wunder/agents/running`：智能体运行状态概览（默认入口 + 个人智能体 + 共享智能体）
  - 返回：`data.total`、`data.items`（agent_id/session_id/updated_at/expires_at/state/pending_question/is_default/last_error?）
  - `is_default`：表示通用聊天（无 agent_id 的默认入口会话）
  - `state`：`idle` | `waiting` | `running` | `cancelling` | `done` | `error`
  - `pending_question`：表示当前会话存在待处理交互（问询面板或工具审批；通常对应 `state=waiting`）
  - `last_error`：仅 `state=error` 时可能返回，表示最近一次错误的摘要（用于前端提示）
- `GET /wunder/agents/user-rounds`：当前用户按智能体汇总的历史用户轮次
  - 返回：`data.total`、`data.items`（agent_id/user_rounds）
  - `agent_id` 为空表示默认入口（通用聊天）
- `POST /wunder/agents`：创建智能体
  - 入参（JSON）：`name`（必填）、`description`（可选）、`system_prompt`（可选）、`tool_names`（可选）、`is_shared`（可选）、`status`（可选）、`approval_mode`（可选：`suggest`/`auto_edit`/`full_auto`）、`icon`（可选）、`sandbox_container_id`（可选，1~10，默认 1）、`hive_id`（可选）、`hive_name`（可选）、`hive_description`（可选）
  - `approval_mode` 兼容别名：`approvalMode`、`approval_mode`、`permissionLevel`、`permission_level`
  - `hive_id` 兼容别名：`hiveId`、`beeroomGroupId`、`beeroom_group_id`
  - `hive_name` 兼容别名：`hiveName`、`beeroomGroupName`、`beeroom_group_name`
  - `hive_description` 兼容别名：`hiveDescription`、`beeroomGroupDescription`、`beeroom_group_description`
  - 当 `hive_name` 有值时，后端会优先按名称自动创建或复用目标蜂群，再把智能体归入该蜂群。
  - 返回：`data`（同智能体详情）
- `GET /wunder/agents/{agent_id}`：智能体详情
  - 返回：`data`（同智能体详情）
  - 默认入口支持：`agent_id` 可传 `__default__`（或 `default`）读取默认入口配置（未配置时返回系统默认值）
- `GET /wunder/agents/{agent_id}/runtime-records`：智能体运行记录（用户侧）
  - Query：`days`（可选，1~90，默认 14，用于日趋势窗口）、`date`（可选，`YYYY-MM-DD`，用于工具调用热力图日期）
  - 返回：`data.agent_id`、`data.range`（`days/start_date/end_date/selected_date`）、`data.summary`（`runtime_seconds/billed_tokens/quota_consumed/tool_calls`，为该智能体累计汇总）、`data.daily[]`（最近 `days` 天按天统计折线图数据）、`data.heatmap`（`date/max_calls/items[]`，`items[].hourly_calls` 为 24 小时调用次数）
  - 默认入口支持：`agent_id` 可传 `__default__`（或 `default`）查看通用聊天入口的运行记录
- `PUT /wunder/agents/{agent_id}`：更新智能体
  - 入参（JSON）：`name`/`description`/`system_prompt`/`tool_names`/`is_shared`/`status`/`approval_mode`/`icon`/`sandbox_container_id`/`hive_id`/`hive_name`/`hive_description`（均可选）
  - 返回：`data`（同智能体详情）
  - 默认入口支持：`agent_id` 为 `__default__`（或 `default`）时更新默认入口配置
- `DELETE /wunder/agents/{agent_id}`：删除智能体
  - 返回：`data.id`
  - 默认入口支持：`agent_id` 为 `__default__`（或 `default`）时清空默认入口配置（恢复系统默认值）
- `GET /wunder/agents/{agent_id}/default-session`：获取智能体主线程会话
  - 返回：`data.agent_id`、`data.session_id`
- `POST /wunder/agents/{agent_id}/default-session`：设置智能体主线程会话
  - 入参（JSON）：`session_id`
  - 返回：`data.agent_id`、`data.session_id`、`data.thread_id`、`data.status`、`data.updated_at`
  - 说明：默认入口使用 `agent_id=__default__`
- 说明：
  - 智能体提示词会追加到基础系统提示词末尾。
  - `tool_names` 会按用户工具白名单过滤。
- 默认入口（`agent_id` 为空或 `__default__/default`）未配置时按当前用户可用工具集兜底（desktop 本地模式默认额外启用 `计划面板`）。
  - `approval_mode` 默认 `auto_edit`，用于控制命令执行/PTC 工具的审批强度。
  - 共享智能体对所有用户可见，管理员可通过单用户权限覆盖进一步调整。

### 4.1.57 `/wunder/beeroom/groups`

- `GET /wunder/beeroom/groups`：列出当前用户全部蜂群分组（接口路径仍沿用 `beeroom` 命名）。
  - Query：`include_archived?`、`mission_limit?`
  - 返回：`data.items[]` / `data.total`，其中每个分组包含 `group_id/hive_id/name/description/status/is_default/agent_total/active_agent_total/idle_agent_total/running_mission_total/mission_total/mother_agent_id/mother_agent_name/members/latest_mission`。
- `POST /wunder/beeroom/groups`：创建蜂群分组。
  - 入参（JSON）：`name`（必填）、`description?`、`group_id?`（兼容 `groupId` / `hive_id` / `hiveId`）、`mother_agent_id?`（兼容 `motherAgentId`）。
  - 返回：`data`（蜂群分组详情摘要）。

### 4.1.58 `/wunder/beeroom/groups/{group_id}`

- `GET /wunder/beeroom/groups/{group_id}`：获取蜂群详情。
  - Query：`mission_limit?`
  - 返回：`data.group`（蜂群摘要）、`data.agents[]`（完整成员态势，包含 `tool_names[]` 便于画布 hover 提示与协作对话栏汇总工具摘要）、`data.missions[]`（最近任务快照，包含 `tasks[]`，可直接驱动画布节点、协作链路边与右侧协作对话栏）。

### 4.1.59 `/wunder/beeroom/groups/{group_id}/move_agents`

- `POST /wunder/beeroom/groups/{group_id}/move_agents`：把若干智能体迁入指定蜂群。
  - 入参（JSON）：`agent_ids`（兼容 `agentIds`）。
  - 返回：`data.moved`、`data.group_id`。

### 4.1.60 `/wunder/beeroom/groups/{group_id}/missions`

- `GET /wunder/beeroom/groups/{group_id}/missions`：按蜂群查询任务列表。
  - Query：`offset?`、`limit?`
  - 返回：`data.items[]` / `data.total`，每条任务包含 `team_run_id/mission_id/hive_id/parent_session_id/entry_agent_id/mother_agent_id/strategy/status/completion_status/task_total/task_success/task_failed/context_tokens_total/context_tokens_peak/model_round_total/started_time/finished_time/elapsed_s/summary/error/updated_time/all_tasks_terminal/all_agents_idle/active_agent_ids/idle_agent_ids/tasks[]`。

### 4.1.61 `/wunder/beeroom/groups/{group_id}/missions/{mission_id}`

- `GET /wunder/beeroom/groups/{group_id}/missions/{mission_id}`：获取单个蜂群任务详情。
  - 后端会校验任务所属用户和 `hive_id` 是否与目标蜂群一致。

### 4.1.62 `/wunder/beeroom/groups/{group_id}/chat/messages`

- `GET /wunder/beeroom/groups/{group_id}/chat/messages`：读取蜂群画布右侧协作对话历史。
  - Query：`before_message_id?`、`limit?`（默认 120，最大 200）
  - 返回：`data.items[]`，每条消息包含 `message_id/group_id/sender_kind/sender_name/sender_agent_id/mention_name/mention_agent_id/body/meta/tone/client_msg_id/created_at`。
- `POST /wunder/beeroom/groups/{group_id}/chat/messages`：追加一条蜂群协作消息。
  - 入参（JSON）：`body`（必填）、`sender_kind?`（兼容 `senderKind`）、`sender_name?`（兼容 `senderName`）、`sender_agent_id?`（兼容 `senderAgentId`）、`mention_name?`（兼容 `mentionName/mention`）、`mention_agent_id?`（兼容 `mentionAgentId`）、`meta?`、`tone?`、`client_msg_id?`（兼容 `clientMsgId`）、`created_at?`（兼容 `createdAt`）。
  - 说明：服务端会按 `user_id + group_id + client_msg_id` 做幂等去重，适合前端乐观追加与重试。
- `DELETE /wunder/beeroom/groups/{group_id}/chat/messages`：清空当前蜂群协作对话历史。
  - 返回：`data.deleted`、`data.group_id`。

### 4.1.63 `/wunder/beeroom/ws`

- `GET /wunder/beeroom/ws`：蜂群协作对话实时 WS 通道（多路复用协议，与主聊天 WS 一致）。
  - 认证：支持 `Authorization: Bearer <token>`；浏览器端可通过 query `access_token/token` 兜底。
  - 客户端消息：
    - `connect`：握手与能力确认；
    - `watch`：开始监听某个蜂群 `group_id`（payload 支持 `after_event_id?`）；
    - `cancel`：取消指定 watch（`target_request_id?`）；
    - `ping`：心跳。
  - 服务端事件：
    - `watching`：已开始监听；
    - `chat_message`：蜂群协作消息追加；
    - `chat_cleared`：协作消息被清空；
    - `team_start`：蜂群任务启动；
    - `team_task_dispatch`：母蜂派发子任务（创建 TeamTask 后即时下发）；
    - `team_task_update`：子任务状态更新（如 `running/cancelled/failed`）；
    - `team_task_result`：子任务结果落盘（含 `result_summary/error/retry_count`）；
    - `team_merge`：母蜂汇总阶段；
    - `team_finish`：蜂群任务闭环完成；
    - `team_error`：蜂群任务异常收敛；
    - `sync_required`：客户端落后（lag）需要全量补齐；
    - `final/error/pong/ready`：协议控制事件。
  - `team_*` 事件公共字段：
    - `team_run_id`：任务 ID（mission ID）；
    - `hive_id`：蜂群 ID；
    - `status`：当前状态；
    - `updated_time`：事件生成时间戳（秒）。

### 4.1.64 `/wunder/beeroom/groups/{group_id}/chat/stream`

- `GET /wunder/beeroom/groups/{group_id}/chat/stream`：蜂群协作对话 SSE 通道（WS 不可用时兜底）。
  - Query：`after_event_id?`、`access_token?`、`token?`
  - Header：可选 `Last-Event-ID`（未传 `after_event_id` 时用作续传游标）
  - 服务端事件：
    - `watching`：流已建立，返回当前游标；
    - `chat_message`：单条消息事件（带 `id=event_id`）；
    - `chat_cleared`：清空事件；
    - `team_start/team_task_dispatch/team_task_update/team_task_result/team_merge/team_finish/team_error`：与 WS 同语义；
    - `sync_required`：消费者 lag 触发补齐提示。
  - 说明：当前事件总线为内存广播，仅保证“在线近实时推送”；断线重连后由前端调用 `GET /chat/messages` 与 `GET /beeroom/groups/{group_id}` 做一致性补齐。

### 4.1.65 蜂群工具与 TeamRun 补充说明

- `agent_swarm send` / `agent_swarm batch_send` 现在会：
  - 自动解析当前蜂巢 `hive_id`；
  - 只允许发现和派发同蜂巢成员；
  - 自动认领/沿用母蜂；
  - 未显式指定目标会话时优先复用目标智能体主线程，仅在该智能体尚无主线程/会话时才新建；
  - 创建 `team_run/team_task` 并回写 `mother_agent_id`、`session_run_id`；
  - 在 `team_run/team_task` 创建、状态更新、终态收敛时同步广播 `team_start/team_task_dispatch/team_task_update/team_task_result/team_finish/team_error` 到 Beeroom WS/SSE；
  - 在实际派发消息中自动附带 `SWARM_CONTEXT`（蜂巢基本信息、母蜂、发送者、活跃成员快照、`team_run_id/task_id` 等）。
- TeamRun / TeamTask 相关返回体新增关键字段：
  - `team_run.mother_agent_id`
  - `team_task.session_run_id`
  - 任务快照 `completion_status`（`running/awaiting_idle/completed/failed/cancelled`）
  - 首次读取智能体列表会按 `config/wunder.yaml` 的 `user_agents.presets` 自动补齐默认智能体，可通过配置调整数量与内容。
  - `sandbox_container_id` 取值范围 1~10，默认 1；同一用户下相同容器编号的智能体共享同一文件工作区。

### 4.1.66 `/wunder/beeroom/packs/*`（已实现，Phase 1）

- 说明：用于“蜂群包（HivePack）/工蜂包（WorkerPack）”资产导入导出；协议细节见 `docs/蜂巢协议设计.md`。
- 任务状态机：
  - `uploaded` → `validating` → `planning` → `installing` → `creating_agents` → `activating` → `completed|failed`
  - 返回体统一包含：`job_id/job_type/status/phase/progress/summary/detail?/report?/artifact?`

- `POST /wunder/beeroom/packs/import`
  - 形态：`multipart/form-data`
  - 字段：
    - `file`：必填，`.hivepack` 或 `.zip`
    - `options`：可选 JSON（支持 `group_id`、`create_hive_if_missing`、`conflict_mode`）
    - `group_id/groupId/hive_id/hiveId`：可选，目标蜂群 ID（与 `options.group_id` 二选一）
  - 行为：
    - 校验包结构与路径安全；
    - 解析 `hive.yaml` 与 `workers/*/skills.yaml`；
    - 支持新结构：`skills/*` 存放技能原始文件，`workers/*/skills.yaml` 仅记录技能名；
    - 支持极简手工包：`hive.yaml + skills/*/SKILL.md + workers/*/WORKER_ROLE.md + workers/*/skills.yaml`；
    - 当 `hive.yaml.workers[]` 为空时，自动扫描 `workers/*` 目录作为工蜂；
    - 当 `workers/*/skills.yaml` 缺失时，兼容读取旧版 `worker.yaml` 或 `workers/*/skills/*/SKILL.md`；
    - `skill.yaml/checksums.sha256/signatures/package.sig` 为可选增强，缺失不阻塞导入；
    - 安装技能包到用户 `custom skills`；
    - 自动创建工蜂智能体并归属蜂群；
    - `conflict_mode=auto_rename_only`（默认）：蜂群/智能体/技能冲突时自动追加后缀并新建，不复用；
    - `conflict_mode=update_replace`：定位目标蜂群后原位替换（保留目标蜂群标识，替换原成员；技能同名按原名覆盖）；
    - 工蜂工具挂载默认包含：系统内置 + MCP + 知识库 + 本次导入技能；
    - 失败时执行回滚（删除已创建智能体、移除本次技能、还原技能启用集；新建蜂群会归档）。
  - 返回：`data`（任务快照）。
  - `report.conflicts`（`status=completed` 时）：
    - `policy`：冲突策略（`auto_rename_only|update_replace`）；
    - `renamed_total`：自动改名总数；
    - `hive.from/to`：蜂群名称与 ID 的改名前后；
    - `agents.renamed_total/renames[]`：工蜂改名统计与明细；
    - `skills.renamed_total/renames[]`：技能改名统计与明细。
  - `report.replace`（`status=completed` 时）：
    - `enabled`：是否启用原位替换；
    - `replaced_agent_total`：被替换移除的原工蜂数量。

- `GET /wunder/beeroom/packs/import/{job_id}`
  - 返回：导入任务快照（仅当前用户可见）。

- `POST /wunder/beeroom/packs/export`
  - 入参（JSON）：
    - `group_id/groupId/hive_id/hiveId`：必填，蜂群 ID
    - `mode`：可选，`full|reference_only`（默认 `full`）
  - 行为：
    - 按蜂群成员生成标准目录；
    - `full` 导出技能完整内容到包根 `skills/*`，`reference_only` 导出占位技能说明；
    - 工蜂目录仅保留 `WORKER_ROLE.md + skills.yaml`（不再导出 `worker.yaml` 与工蜂内 skills 目录）；
    - 自动生成 `hive.yaml/workers/*/skills.yaml`，并附加可选增强元数据 `skill.yaml/checksums.sha256`；
    - 产出 `.zip` 文件并回写任务产物信息（文件名默认含蜂群名称+时间戳）。
  - 返回：`data`（任务快照）。

- `GET /wunder/beeroom/packs/export/{job_id}`
  - 返回：导出任务快照（仅当前用户可见）。

- `GET /wunder/beeroom/packs/export/{job_id}/download`
  - 说明：仅当导出任务 `status=completed` 可下载。
  - 返回：`.zip` 文件流（`Content-Type: application/zip`）。
- 用户侧 UI（Messenger/蜂群）已接入：
  - 中栏 `+` 直接选择 `.hivepack` 并发起导入；
  - 蜂群列表条目 hover 浮出导出按钮并触发 `full` 导出；
  - 当前中栏导入入口默认走 `auto_rename_only`；`update_replace` 通过导入 options/API 使用；
  - 导入成功时若有改名冲突，会提示“自动改名 N 项”，并弹窗展示改名前后明细（首批）；
  - 导出完成后自动下载产物；`reference_only` 当前通过 API 使用（前端入口待补齐）。

### 4.1.57 `/wunder/prompt_templates`（用户侧）

- `GET /wunder/prompt_templates`：获取当前用户提示词模板包状态
  - 返回：`data.active`、`data.default_sync_pack_id`、`data.packs_root`、`data.packs[]`（`id/is_default/readonly/path/sync_pack_id?`）、`data.segments[]`（`key/file`）
  - 说明：`default` 模板包为只读，内容同步自当前系统启用模板（`prompt_templates.active`）。
- `POST /wunder/prompt_templates/active`：设置当前用户启用模板包
  - 入参（JSON）：`active`（可选，默认 `default`）
  - 返回：`data.active`
- `GET /wunder/prompt_templates/file`：读取模板分段文件
  - Query：`pack_id`（可选，默认当前用户 active）、`locale`（可选，`zh|en`）、`key`（必填：`role/engineering/tools_protocol/skills_protocol/memory/extra`）
  - 返回：`data.pack_id/locale/key/path/exists/fallback_used/readonly/source_pack_id/content`
  - 说明：当自建模板包缺少某分段时，先回退系统启用模板；若系统启用模板也缺失，再回退系统 `default` 模板（`fallback_used=true`，`source_pack_id` 标记实际来源）。
- `PUT /wunder/prompt_templates/file`：写入模板分段文件
  - 入参（JSON）：`pack_id`（可选，默认当前用户 active）、`locale`（可选）、`key`（必填）、`content`（必填）
  - 返回：`data.pack_id/locale/key/path`
  - 说明：`default` 模板包只读，不允许写入。
- `POST /wunder/prompt_templates/packs`：创建用户模板包
  - 入参（JSON）：`pack_id`（必填，仅字母/数字/_/-）、`copy_from`（可选，默认 `default`，支持从当前用户已有包复制）
  - 返回：`data.pack_id/path/copied_from`
- `DELETE /wunder/prompt_templates/packs/{pack_id}`：删除用户模板包
  - 返回：`data.pack_id`
  - 说明：删除当前 active 包时会自动回退到 `default`。

### 4.1.58 `/wunder/admin/user_accounts/*`

- `GET /wunder/admin/user_accounts`
  - Query：`keyword`、`offset`、`limit`
  - 返回：`data.total`、`data.items`（UserProfile + `active_sessions`/`online`/`last_seen_at` + `daily_quota`/`daily_quota_used`/`daily_quota_remaining`/`daily_quota_date`）
- `POST /wunder/admin/user_accounts`
  - 入参（JSON）：`username`、`email`（可选）、`password`、`unit_id`（可选）、`roles`（可选）、`status`（可选）、`is_demo`（可选）
  - 返回：`data`（UserProfile）
- `POST /wunder/admin/user_accounts/test/seed`
  - 入参（JSON）：`per_unit`（每单位新增数量，1~200）
  - 返回：`data.created`、`data.unit_count`、`data.per_unit`、`data.password`
- `POST /wunder/admin/user_accounts/test/cleanup`
  - 返回：`ok`、`prefix`、`matched`、`deleted_users`、`failed`、`failed_items`（按当前管理员可见范围批量删除测试用户）
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

- 定位：管理员侧仅用于渠道运行态监控（只读），不提供渠道账号/绑定写入。
- `GET /wunder/admin/channels/accounts`
  - Query：`channel`、`status`
  - 返回：`data.items`（channel/account_id/config/status/created_at/updated_at/runtime）
  - `runtime.feishu_long_connection`：飞书账号运行态（`running/missing_credentials/disabled/account_inactive/not_configured`）与 `binding_count`
  - `runtime.xmpp_long_connection`：XMPP 账号运行态（`running/missing_credentials/disabled/account_inactive/not_configured`）与 `binding_count`

- `GET /wunder/admin/channels/bindings`
  - Query：`channel`
  - 返回：`data.items`（binding_id/channel/account_id/peer_kind/peer_id/agent_id/tool_overrides/priority/enabled/created_at/updated_at）

- `GET /wunder/admin/channels/user_bindings`
  - Query：`channel`、`account_id`、`peer_kind`、`peer_id`、`user_id`、`offset`、`limit`
  - 返回：`data.items`（channel/account_id/peer_kind/peer_id/user_id/created_at/updated_at）与 `data.total`

- `GET /wunder/admin/channels/sessions`
  - Query：`channel`、`account_id`、`peer_id`、`session_id`、`offset`、`limit`
  - 返回：`data.items`（channel/account_id/peer_kind/peer_id/thread_id/session_id/agent_id/user_id/tts_enabled/tts_voice/metadata/last_message_at/created_at/updated_at）与 `data.total`

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
- `xmpp.*`：XMPP 客户端配置（`jid/password/password_env/domain/host/server/port/direct_tls/trust_self_signed/resource/muc_nick/muc_rooms/rooms/long_connection_enabled/send_initial_presence/status_text/heartbeat_enabled/heartbeat_interval_s/heartbeat_timeout_s/respond_ping`）

### 4.1.60 `/wunder/channels/*`

- 鉴权：必须携带用户侧 token（`Authorization: Bearer <user_token>`）。
- 用户侧渠道账号仅由当前用户维护，和管理侧渠道账号隔离；管理侧页面仅用于运行态监控。

- `GET /wunder/channels/accounts`
  - Query：`channel`（可选，按渠道过滤，如 `feishu`）
  - 返回：
    - `data.items`：当前用户的渠道账号列表（`channel/account_id/name/status/active/meta/config/raw_config/created_at/updated_at`）
    - `data.supported_channels`：前端可用的渠道目录列表（由 `src/channels/catalog.rs` 生成），每项包含：
      - `channel`：渠道标识
      - `display_name`：展示名称
      - `description`：渠道说明
      - `webhook_mode`：接入模式（如 `specialized+generic` / `generic`）
      - `docs_hint`：推荐 webhook 路径提示
    - 当前默认支持：`feishu`、`wechat`、`wechat_mp`、`qqbot`、`whatsapp`、`telegram`、`discord`、`slack`、`line`、`dingtalk`、`xmpp`
  - `meta` 关键字段：
    - `configured`：是否已完成可用配置
    - `peer_kind`：默认会话类型（如 `group` / `user`）
    - `receive_group_chat`：是否接收群聊（飞书）
    - `long_connection_enabled`：是否启用长连接（飞书/XMPP）

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
  - 企业微信快捷入参：
    - `wechat.corp_id`、`wechat.agent_id`、`wechat.secret`（必填或沿用已有值）
    - `wechat.token`、`wechat.encoding_aes_key`（回调签名/解密，可选但建议配置）
    - `wechat.domain`（可选，默认 `qyapi.weixin.qq.com`）
  - 公众号快捷入参：
    - `app_id`、`app_secret` 或 `wechat_mp.app_id`、`wechat_mp.app_secret`（必填或沿用已有值）
    - `wechat_mp.token`、`wechat_mp.encoding_aes_key`（回调签名/解密，可选但建议配置）
    - `wechat_mp.original_id`（用于 ToUserName 账号匹配，可选）
    - `domain` 或 `wechat_mp.domain`（可选，默认 `api.weixin.qq.com`）
    - `peer_kind` 固定 `user`
- XMPP 快捷入参：
    - `xmpp.jid`（必填或沿用已有值）
    - `xmpp.password` 或 `xmpp.password_env`（二选一，缺失时沿用已有值）
    - `xmpp.domain`（可选，SRV 域名或手动服务器域名）
    - `xmpp.host`（兼容别名：`xmpp.server`）、`xmpp.port`、`xmpp.direct_tls`、`xmpp.trust_self_signed`（可选，手动连接地址）
    - `xmpp.resource`（可选，登录资源）
    - `xmpp.muc_nick`、`xmpp.muc_rooms`（兼容别名：`xmpp.rooms`，可选，群聊昵称与自动入群房间）
    - `xmpp.long_connection_enabled`、`xmpp.send_initial_presence`、`xmpp.status_text`（可选，长连接与 presence 策略）
    - `xmpp.heartbeat_enabled`、`xmpp.heartbeat_interval_s`、`xmpp.heartbeat_timeout_s`、`xmpp.respond_ping`（可选，心跳兼容策略）
    - `peer_kind` 默认 `user`
  - 行为说明：
    - 首次创建会自动写入 `inbound_token`。
    - 会自动维护默认绑定（`peer_id="*"`），并按 `peer_kind` / `receive_group_chat` 更新。
    - 传入 `agent_id` 时，默认绑定会同时写入该 `agent_id` 以隔离到指定智能体。
    - 飞书/XMPP 账号保存成功后会以 `long_connection_enabled=true` 参与长连接调度（可手动关闭）。

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
  - 当 `data.stage=tool_failure_guard` 时，会附带 `tool/repeat_count/threshold/tool_error`，表示同类工具连续失败触发止损。
- `event: llm_request`：模型 API 请求体（调试用；默认仅返回基础元信息并标记 `payload_omitted`，开启 `debug_payload` 或日志级别为 debug/trace 时包含完整 payload；若上一轮包含思考过程，将在 messages 中附带 `reasoning_content`；当上一轮为工具调用时，messages 会包含该轮 assistant 原始输出与 reasoning）
  - 说明：`freeform_call + OpenAI Responses` 场景下，`payload.tools` 会同时出现 `type=function` 与 `type=custom`；`apply_patch` 会以原生 grammar tool 暴露，而不是退化成仅靠提示词约束的 XML 调用。
- `event: knowledge_request`：知识库检索模型请求体（调试用，包含 `query` 或 `keywords`、`limit`、`embedding_model` 等）
- `event: llm_output_delta`：模型流式增量片段（调试用，`data.delta` 为正文增量，`data.reasoning_delta` 为思考增量，需按顺序拼接）
  - 说明：断线续传回放时可能携带 event_id_start/event_id_end 用于标记合并范围。
- `event: llm_stream_retry`：流式断线重连提示（`data.attempt/max_attempts/delay_s` 说明重连进度，`data.will_retry=false` 或 `data.final=true` 表示已停止重连，`data.reset_output=true` 表示应清理已拼接的输出）
- `event: llm_output`：模型原始输出内容（调试用，`data.content` 为正文，`data.reasoning` 为思考过程，流式模式下为完整聚合结果）
- `event: token_usage`：单轮模型 token 统计（input_tokens/output_tokens/total_tokens；含 `user_round/model_round`）
- `event: context_usage`：上下文占用量估算（data.context_tokens/message_count，含 `user_round/model_round`）
- `event: quota_usage`：额度消耗统计（每次模型调用都会触发；`data.consumed` 为本次消耗次数，`data.daily_quota/used/remaining/date` 为每日额度状态，含 `user_round/model_round`）
- `event: tool_call`：工具调用信息（名称、参数）
- `event: tool_output_delta`：工具执行输出增量（`data.tool`/`data.command`/`data.stream`/`data.delta`）
  - 说明：当前仅内置“执行命令”在本机模式会输出该事件，沙盒执行不流式返回。
- `event: tool_result`：工具执行结果（data.meta.duration_ms/truncated/output_chars/exit_code/policy）
  - 执行命令工具结果新增 `data.results[].output_meta`（分 stdout/stderr 记录 `total_bytes/omitted_bytes/truncated/head_bytes/tail_bytes`）；同时 `data.meta.output_guard` 提供本次命令批次汇总（`commands/truncated_commands/total_bytes/omitted_bytes`），用于大输出场景的稳态消费。
- `event: workspace_update`：工作区变更事件（data.workspace_id/agent_id/container_id/tree_version/tool/reason/path/changed_paths）
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
  - `reason` 取值：`history/overflow/hard_guard`；其中 `hard_guard` 表示触发了上下文硬阈值守卫压缩（用于无 `max_context` 或 `max_context` 过高导致上下文膨胀的场景）。
  - `status` 新增 `guard_only`：表示当前轮缺少可摘要历史（常见于首轮超长输入），系统仅执行 `context_guard` 直接裁剪当前消息后继续调用模型。
  - 关键字段：`hard_guard_triggered/context_guard_applied/context_guard_tokens_before/context_guard_tokens_after/context_guard_current_user_trimmed/context_guard_summary_trimmed/context_guard_summary_removed`，用于说明压缩触发来源及压缩后为适配上下文窗口执行的二次裁剪动作。
- `event: final`：最终回复（`data.answer`/`data.usage`/`data.stop_reason`）
  - `stop_reason` 取值：`model_response`（模型直接回复）、`final_tool`（最终回复工具）、`a2ui`（A2UI 工具）、`question_panel`（等待问询面板选择）、`tool_failure_guard`（检测到连续工具失败并主动止损）、`max_rounds`（达到最大轮次兜底）、`empty_response`（模型未返回可展示最终答复）、`unknown`（兜底）
  - 当 `stop_reason=tool_failure_guard` 时，`data.stop_meta` 会包含 `type/tool/repeat_count/threshold/tool_error`，便于前端明确提示“已达到失败保护阈值并停止自动重试”。
  - 当 `stop_reason=max_rounds` 时，`data.answer` 会优先返回面向用户的恢复提示，引导用户直接继续当前会话，或调大该模型的 `max_rounds` 后重试；若本轮已生成中间文件/表格，用户也可先查看工作区结果。
  - 常规 `data.answer` 输出也会在展示前移除工具调用片段与 `<think>...</think>` 推理标签，避免把模型中间思考直接暴露给用户。
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
- Queue-full protection: when the current WS outbound queue is full, the server stops live push for that request; clients should use `resume` + `after_event_id` to replay pending events
- 多路复用：同一连接可并发多个请求，需设置 `request_id`；服务端 `event/error` 会回传对应 `request_id`
- `type=error` 统一错误载荷字段：`code`/`message`/`status`/`hint`/`trace_id`/`timestamp`；若为结构化请求错误，额外带 `detail`（例如 `field`/`max_chars`/`actual_chars`）。
- 断线续传：客户端发送 `resume` + `after_event_id`，服务端从 `stream_events` 回放并继续推送
- 实时订阅：客户端发送 `watch` + `after_event_id`，服务端持续推送会话流事件（直到取消或断线）
- 审批回传：客户端可发送 `type=approval` 响应审批请求（`payload.approval_id`、`payload.decision=approve_once|approve_session|deny`，可选 `session_id`）
- 审批事件：服务端会在流中发送 `event=approval_request` 与 `event=approval_result`
- 详细协议与节点说明：见 `docs/方案/WebSocket-Transport.md`

