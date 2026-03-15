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
- docker compose 默认使用两个命名卷：`wunder_workspaces` 挂载到 `/workspaces`（用户工作区）；`wunder_logs` 挂载到 PostgreSQL/Weaviate 数据目录（`/var/lib/postgresql/data`、`/var/lib/weaviate`）；`/wunder/temp_dir/*` 默认落在本地 `./temp_dir`（容器内 `/app/temp_dir`，可用 `WUNDER_TEMP_DIR_ROOT` 覆盖）；运行态可写配置保留在仓库本地 `data/`（`data/config`、`data/prompt_templates`、`data/user_tools` 等）。构建/依赖缓存（`target/`、`.cargo/`、根 `node_modules/`）保持写入仓库目录便于管理；Ubuntu20 Desktop 打包服务默认额外挂载并复用 `target/x86-20/.cache` / `target/arm64-20/.cache` 里的 npm、Electron 与 electron-builder 缓存，便于首次在线构建后迁入内网继续复构；前端开发容器不再额外挂载 `frontend/node_modules` 与 `desktop/electron/node_modules` 的遮罩卷，两处目录应保持为空或不存在；同时前端开发容器仅安装 `wunder-frontend` workspace 依赖，避免在前端调试阶段触发 `desktop/electron` 的 `electron` 下载脚本。
- `wunder-frontend` 在 docker compose 中会先构建到临时目录 `frontend/dist.__docker_tmp`，再按“资源文件优先、`index.html` 最后切换”的顺序同步到 `frontend/dist`；frontend 健康检查只判断 `dist/index.html` 是否就绪，`wunder-nginx` 仅依赖 frontend 已启动并在自身入口等待静态入口文件，避免首启时因后端仍在编译而把 frontend 误判为启动失败。
- 沙盒服务：独立容器运行 `wunder-server` 的 `sandbox` 模式（`WUNDER_SERVER_MODE=sandbox`），对外提供 `/sandboxes/execute_tool` 与 `/sandboxes/release`，由 `WUNDER_SANDBOX_ENDPOINT` 指定地址。
- 工具清单与提示词注入复用统一的工具规格构建逻辑：`tool_call/freeform_call` 模式会注入工具协议片段，`function_call` 模式不注入工具提示词，工具清单仅用于 tools 协议。
- 当 `tool_call_mode=freeform_call` 且模型走 OpenAI Responses API 时，服务端会把 `apply_patch` 这类语法工具下发为原生 `type=custom` 工具（携带 `format={type:grammar,syntax:lark,definition}`），普通 JSON 工具继续走 `type=function`；工具结果会按 `custom_tool_call_output/function_call_output` 回填历史，避免仅靠 XML 提示词驱动。
- 配置分层：基础配置优先读取 `config/wunder.yaml`（`WUNDER_CONFIG_PATH` 可覆盖）；若不存在则自动回退 `config/wunder-example.yaml`；管理端修改会写入 `data/config/wunder.override.yaml`（`WUNDER_CONFIG_OVERRIDE_PATH` 可覆盖）。
- 环境变量：`.env` 为可选项；docker compose 通过 `${VAR:-default}` 提供默认值，未提供 `.env` 也可直接启动。
- compose 镜像策略：`docker-compose-x86.yml` / `docker-compose-arm.yml` 的 `wunder-server` / `wunder-sandbox` / `extra-mcp` 统一使用同名本地镜像（`wunder-x86`/`wunder-arm`），并设置 `pull_policy: never`，已存在镜像时优先复用，不存在时再自动构建，避免首次启动时 `extra-mcp` 先拉取失败。
- 前端入口：管理端调试 UI `http://127.0.0.1:18000`，调试前端 `http://127.0.0.1:18001`（Vite dev server），用户侧前端 `http://127.0.0.1:18002`（Nginx 静态服务）。
- Single-port docker compose mode: expose only `18001` publicly; proxy `/wunder`, `/a2a`, and `/.well-known/agent-card.json` to `wunder-server:18000`; keep `wunder-postgres`/`wunder-weaviate`/`extra-mcp` bound to `127.0.0.1`.
- 鉴权：管理员接口使用 `X-API-Key` 或 `Authorization: Bearer <api_key>`（配置项 `security.api_key`），用户侧接口使用 `/wunder/auth` 颁发的 `Authorization: Bearer <user_token>`；外部系统嵌入接入使用 `security.external_auth_key`（环境变量 `WUNDER_EXTERNAL_AUTH_KEY`）调用 `/wunder/auth/external/*`。当未显式配置 `external_auth_key` 时会自动回退到 `security.api_key`，即默认启用外链鉴权；并可通过 `security.external_embed_preset_agent_name` 限定外链默认智能体。
- 默认管理员账号为 admin/admin，服务启动时自动创建且不可删除，可通过用户管理重置密码。
- 用户端请求可省略 `user_id`，后端从 Token 解析；管理员接口可显式传 `user_id` 以指定目标用户。
- 模型配置新增 `model_type=llm|embedding`，向量知识库依赖 embedding 模型调用 `/v1/embeddings`。
- 用户侧前端默认入口为 `/app/home`（desktop 为 `/desktop/home`）；`/app/home|chat|user-world|workspace|tools|settings|profile|channels|cron` 统一复用 Messenger 壳。嵌入聊天路由为 `/app/embed/chat`（desktop `/desktop/embed/chat`，demo `/demo/embed/chat`，隐藏左/中栏）。外链详情路由为 `/app/external/:linkId`（demo 为 `/demo/external/:linkId`）。External links are managed via `/wunder/admin/external_links` and delivered by `/wunder/external_links` after org-level filtering; production frontend port is 18002, development port is 18001。
- 当使用 API Key/管理员 Token 访问 `/wunder`、`/wunder/chat`、`/wunder/workspace`、`/wunder/user_tools` 时，`user_id` 允许为“虚拟用户”，无需在 `user_accounts` 注册，仅用于线程/工作区/工具隔离。
- 工作区容器约定：用户私有容器固定为 `container_id=0`，智能体容器范围为 `1~10`；`/wunder/workspace*` 全部接口（含 upload）支持显式 `container_id`，且优先级高于 `agent_id` 推导。
- Desktop 本地模式下，这些容器默认映射到本地持久目录，不执行“24 小时自动清理”策略；用户文件需显式删除。内置文件工具在本地模式下还支持直接访问本机绝对路径，不再强制限制在工作区内。
- Desktop 本地模式的 `/wunder/desktop/settings` 新增 `python_interpreter_path` 字段：留空时优先使用安装包内置 Python，填写有效解释器路径后运行时优先切换到用户自定义 Python。
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
- 新增内置工具 `渠道工具`（英文别名 `channel_tool`），通过 `action=list_contacts|send_message` 查询渠道可联系对象并向指定渠道对象发送消息（支持工作区文件引用转下载链接后发送）。
- `渠道工具.list_contacts` 默认融合会话历史与 XMPP roster（若可用），返回 `source=session_history|roster|session_history+roster`；可传 `refresh=true` 强制刷新 roster 缓存。
- `渠道工具.send_message` 参数已简化：不再强制 `channel/account_id/to` 同时必填；可直接传 `text`（或 `content`/`attachments`）并由系统从会话/默认账号自动补全。`list_contacts` 返回 `contact` 对象，可直接回传给 `send_message`。
- 测试开放态（2026-03-11）：`channel_tool` 默认放开账号归属限制，`list_contacts` 可读取当前系统内所有已配置渠道账号；渠道请求默认覆盖 `security.approval_mode=full_auto` 与 `security.exec_policy_mode=allow`，不再进入渠道审批提示链路。
- 新增内置工具 `浏览器`（英文别名 `browser`），通过 `action=navigate|click|type|screenshot|read_page|close` 统一操作，仅 desktop 模式可用。
- 新增内置工具 `桌面控制器`（英文别名 `desktop_controller`/`controller`），通过 bbox+action 执行桌面操作，执行后自动附加桌面截图，仅 desktop 模式可用。
- 新增内置工具 `桌面监视器`（英文别名 `desktop_monitor`/`monitor`），等待 wait_ms 后返回桌面截图并自动附加，仅 desktop 模式可用。
- `桌面控制器/桌面监视器` 在同一会话内会额外返回 `previous_screenshot_path`；工具 followup 会按“上一帧 -> 当前帧”顺序自动回灌图片（首帧仅回灌当前帧）。
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
  - `security.external_embed_preset_agent_name`：外链嵌入预制智能体名称（为空表示未配置）
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
- 吞吐量/性能/benchmark/模拟：`/wunder/admin/throughput/*`、`/wunder/admin/performance/sample`、`/wunder/admin/benchmark/*`、`/wunder/admin/sim_lab/*`。
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
- 当前推荐方式：通过结构化记忆碎片系统 + 可选内置工具 `记忆管理`（`memory_manager`）协同维护长期记忆。
- 作用域：按 `用户 + 智能体` 隔离；记忆只在线程首次建立时注入到系统提示词快照，同一线程后续不再自动改写系统提示词，如需读取最新记忆请通过 `memory_manager` 的 `recall` 动作主动检索。
- recall 命中解释会输出 `matched_terms`、`matched_fields`、`phrase_match`、`category_match`、置顶/近期等原因字段；当系统存在可用 embedding 模型时，还会以 best-effort 方式追加语义 rerank，并在命中记录中写入 `semantic_score` 与相关原因字段。记忆碎片 embedding 会按 `memory_id + embedding_model + content_hash` 持久化缓存，避免后续 recall 重复为同一碎片做 embedding。
- 记忆碎片当前可见状态为 `active / superseded / invalidated`；其中 `superseded` 表示该碎片已被同 `fact_key` 的新版本替代，默认不会被 recall 返回，但仍会在列表接口与用户可视化卡片墙中展示。
- recall 命中、碎片创建/编辑、列表读取时会惰性刷新 `tier(core/working/peripheral)` 与状态链路；因此接口返回的 `tier`、`status`、`supersedes_memory_id`、`superseded_by_memory_id` 字段可直接用于前端展示版本关系与生命周期信息。
- `memory_manager` 的 `list/add/update/delete/clear/recall` 已与结构化 `memory_fragments` 共用同一条主存储链路；模型经工具写入的新记忆会直接出现在用户侧“记忆碎片”卡片页，无需再等待旧摘要表懒迁移。
- `confirmed_by_user` 字段当前仅作为兼容旧数据保留，不再作为用户侧记忆碎片页面的交互入口，也不再参与 recall 排序和提示词快照构建。
- 聊天页提示词预览接口 `/wunder/chat/system-prompt` 与 `/wunder/chat/sessions/{session_id}/system-prompt` 现会额外返回 `memory_preview`、`memory_preview_mode(frozen/pending/none)`、`memory_preview_count`，用于向用户明确展示“当前线程已冻结”或“新线程将注入”的记忆快照。

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

### 4.1.48 `/wunder/admin/benchmark/*`

- 旧 `/wunder/admin/evaluation/*` 能力评估接口已移除。
- 当前统一使用 PinchBench 风格的 `/wunder/admin/benchmark/*` 基准测试接口，详见下文 `PinchBench Benchmark API`。

### PinchBench Benchmark API

#### `GET /wunder/admin/benchmark/suites`
- 方法：`GET`
- 返回（JSON）：`{ "suites": [...] }`
- 每个 suite 项包含 `suite_id`、`task_count`、`categories`、`grading_types`、`recommended_runs`，用于管理端快速构建套件筛选与推荐轮次。

#### `GET /wunder/admin/benchmark/tasks`
- 方法：`GET`
- 入参（Query，可选）：`suite`、`category`、`grading_type`。
- 返回（JSON）：`{ "tasks": [...] }`。每个任务项包含 `id`、`name`、`suite`、`category`、`grading_type`、`timeout_seconds`、`runs_recommended`、`difficulty`、`required_tools`、`tags`、`languages`、`criteria_count`、`has_automated_checks`、`has_judge_rubric`、`prompt`、`expected_behavior`。

#### `POST /wunder/admin/benchmark/start`
- 方法：`POST`
- 入参（JSON）：`user_id`（必填）、`model_name`、`judge_model_name`、`suite_ids`、`task_ids`、`runs_per_task`、`capture_artifacts`、`capture_transcript`、`tool_names`、`config_overrides`。
- 返回（JSON）：`run_id`、`status`、`task_count`、`attempt_count`、`suite_ids`。启动后服务端会异步执行 benchmark。

#### `GET /wunder/admin/benchmark/runs`
- 方法：`GET`
- 入参（Query，可选）：`user_id`、`status`、`model_name`、`since_time`、`until_time`、`limit`。
- 返回（JSON）：`{ "runs": [...] }`，每项为 benchmark 运行快照，包含运行状态、总分、suite 列表、效率摘要、开始/结束时间等信息。

#### `GET /wunder/admin/benchmark/runs/{run_id}`
- 方法：`GET`
- 返回（JSON）：`{ "run": ..., "tasks": [...], "attempts": [...] }`。其中 `tasks` 为任务聚合结果，`attempts` 为逐轮明细，便于历史回放与问题定位。

#### `POST /wunder/admin/benchmark/runs/{run_id}/cancel`
- 方法：`POST`
- 返回（JSON）：`{ "ok": true, "run_id": "...", "message": "cancel requested" }`。仅对仍在内存中的运行实例生效。

#### `DELETE /wunder/admin/benchmark/runs/{run_id}`
- 方法：`DELETE`
- 返回（JSON）：`{ "ok": true, "run_id": "...", "deleted": N }`。会同时删除该 run 关联的 attempt 与 task aggregate 持久化结果。

#### `GET /wunder/admin/benchmark/runs/{run_id}/stream`
- 方法：`GET`（SSE）
- 返回：SSE 事件流；事件名包括 `benchmark_started`、`task_attempt_started`、`task_attempt_finished`、`task_aggregated`、`benchmark_progress`、`benchmark_log`、`benchmark_finished`。
- 说明：仅能订阅仍在运行中的 benchmark；运行结束后需要改用明细接口查询最终结果。
