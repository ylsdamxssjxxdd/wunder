# wunder API 文档

## 4. API 设计

### 4.0 实现说明

- 接口实现基于 Rust Axum，路由拆分在 `src/api`（core/chat/user_world/user_tools/user_agents/user_channels/admin/a2a/desktop 等模块）。
- 当前产品核心能力采用“五维能力框架”：**形态协同 / 租户治理 / 智能体协作 / 工具生态 / 接口开放**；用户体系聊天（用户↔智能体 + 用户↔用户）是默认主线。
- 运行与热重载环境建议使用 `Dockerfile` + `docker-compose-x86.yml`/`docker-compose-arm.yml`；Windows 本地开发如果 bind mount 导致前端/编译 I/O 明显卡顿，可改用新增的 `docker-compose-win.yml`，保留源码目录挂载并把运行态数据、前端产物和依赖缓存切到 Docker named volume。
- MCP 服务容器：`extra-mcp` 用于运行 `extra_mcp/` 下的 FastMCP 服务脚本，默认以 streamable-http 暴露端口，人员数据库连接通过 `config/mcp_config.json` 的 `database` 配置。
- MCP 配置文件：`config/mcp_config.json` 支持集中管理人员数据库配置，可通过 `MCP_CONFIG_PATH` 指定路径，数据库配置以配置文件为准；默认优先读取该路径，不存在时兼容回退到 `extra_mcp/mcp_config.json`。
- 多数据库支持：在 `mcp_config.json` 的 `database.targets` 中配置多个数据库（MySQL/PostgreSQL），默认使用 `default_key`，需要切换目标可调整 `default_key` 或部署多个 MCP 实例。
- Database data tools: configure `database.tables` (or `database.query_tables`) to auto-register table-scoped `db_query` + `db_export` tools (`db_query`/`db_export` for single table, `db_query_<key>`/`db_export_<key>` for multiple). Each tool is hard-bound to its table; `db_query*` embeds compact schema hints (`column + type`) in description and returns `query_handle`, while `db_export*` writes xlsx/csv directly under the configured export root (`database.export_root`) or, when `path` points to `/workspaces/{user_id}/...`, directly into the current Wunder workspace and returns a lean export payload centered on canonical `path` plus `workspace_relative_path` for follow-up tools. By default `db_export*` rejects SQL/query_handle that still contains `LIMIT/OFFSET`; set `allow_limited_export=true` only for intentional partial exports.
- 单库类型切换：设置 `database.db_type=mysql|postgres`，或在多库配置中为每个目标指定 `type/engine` 或 DSN scheme。
- 知识库 MCP：按 `knowledge.targets` 动态注册 `kb_query` 工具（单目标为 `kb_query`，多目标自动命名为 `kb_query_<key>`）；向量知识库检索不依赖 RAGFlow MCP。
- 向量知识库使用 Weaviate，连接参数位于 `vector_store.weaviate`（url/api_key/timeout_s/batch_size）。
- docker compose 默认将运行态持久化统一落在仓库 `config/data/`：`./config/data/workspaces` 挂载到 `/workspaces`（用户工作区）、`./config/data/postgres` 挂载到 PostgreSQL 数据目录、`./config/data/weaviate` 挂载到 Weaviate 数据目录；服务内部的 SQLite fallback、用户提示词模板、`temp_dir`、`vector_knowledge`、吞吐报告与 monitor 历史默认路径也统一收口到 `config/data/`，避免在仓库根目录再生成 `data/`、`temp_dir/`、`vector_knowledge/`。主配置文件直接使用仓库 `config/wunder.yaml`（容器内默认 `/app/config/wunder.yaml`，可通过 `WUNDER_CONFIG_PATH` 改到其他单文件路径）；`WUNDER_USER_TOOLS_ROOT` / `WUNDER_VECTOR_KNOWLEDGE_ROOT` / `WUNDER_TEMP_DIR_ROOT` 默认也已对齐到 `/app/config/data/*`。构建/依赖缓存（`target/`、`.cargo/`、根 `node_modules/`）保持写入仓库目录便于管理；Ubuntu20 Desktop 打包服务默认额外挂载并复用 `target/x86-20/.cache` / `target/arm64-20/.cache` 里的 npm、Electron 与 electron-builder 缓存，便于首次在线构建后迁入内网继续复构；前端开发容器不再额外挂载 `frontend/node_modules` 与 `desktop/electron/node_modules` 的遮罩卷，两处目录应保持为空或不存在；同时前端开发容器仅安装 `wunder-frontend` workspace 依赖，避免在前端调试阶段触发 `desktop/electron` 的 `electron` 下载脚本。`docker-compose-win.yml` 额外用 `wunder_win_data` 兜底整个 `/app/config/data`，并对 `workspaces/browser/user_tools/vector_knowledge/temp_dir` 等热点目录继续做子卷覆盖，尽量避免 Windows 宿主仓库继续承接运行态写入。
- 前端多平台依赖目录约定：仓库根使用并行 profile 保存不同系统的依赖树，当前默认包括 `node_modules-win-x86/`、`node_modules-linux-x86/`、`node_modules-linux-arm/`；根 `node_modules/` 只作为当前宿主平台的活动入口（链接/联接点），宿主机可通过 `python scripts/node_modules_profile.py status|use|adopt ...` 管理。`docker-compose-x86.yml` 会把 `./node_modules-linux-x86` 挂到 `/workspace/node_modules`，`docker-compose-arm.yml` 会把 `./node_modules-linux-arm` 挂到 `/workspace/node_modules`，从而避免 Linux 容器内的 `npm ci` 改写宿主机 Windows 依赖目录。
- `wunder-frontend` 在 docker compose 中会先构建到临时目录 `frontend/dist.__docker_tmp`，再按“资源文件优先、`index.html` 最后切换”的顺序同步到 `frontend/dist`；构建阶段直接调用 `vite/bin/vite.js`，并按真实文件标记校验 Linux 容器内的 `rollup`/`esbuild` 平台原生依赖，避免目录存在但实际为空壳时误判为可用；ARM compose 默认关闭 `FRONTEND_ALLOW_PREBUILT_DIST`，优先要求真实 ARM `node_modules` 与真实构建产物，只有显式设为 `1` 时才允许复用现有静态产物兜底。
- `docker-compose-arm.yml` 的 `wunder-server` 与 `wunder-sandbox` 默认注入 `WUNDER_PREFER_PREBUILT_BIN=0`：ARM 环境默认按源码/产物时间关系正常判定是否需要重新构建；如需显式优先复用既有 ARM release 二进制，可在 `.env` 中设置 `WUNDER_PREFER_PREBUILT_BIN=1`。
- 沙盒服务：独立容器运行 `wunder-server` 的 `sandbox` 模式（`WUNDER_SERVER_MODE=sandbox`），对外提供 `/sandboxes/execute_tool` 与 `/sandboxes/release`，由 `WUNDER_SANDBOX_ENDPOINT` 指定地址。
- 工具清单与提示词注入复用统一的工具规格构建逻辑：`tool_call/freeform_call` 模式会注入工具协议片段，`function_call` 模式不注入工具提示词，工具清单仅用于 tools 协议。
- 当 `tool_call_mode=freeform_call` 且模型走 OpenAI Responses API 时，服务端会把 `apply_patch` 这类语法工具下发为原生 `type=custom` 工具（携带 `format={type:grammar,syntax:lark,definition}`），普通 JSON 工具继续走 `type=function`；工具结果会按 `custom_tool_call_output/function_call_output` 回填历史，避免仅靠 XML 提示词驱动。
- 配置加载：运行时只读取单一配置文件 `config/wunder.yaml`（`WUNDER_CONFIG_PATH` 可覆盖）；若不存在则自动回退 `config/wunder-example.yaml`；管理端修改会直接写回当前生效的配置文件，不再额外维护独立覆盖层。
- 环境变量：`.env` 为可选项；docker compose 通过 `${VAR:-default}` 提供默认值，未提供 `.env` 也可直接启动。
- compose 镜像策略：`docker-compose-x86.yml` / `docker-compose-arm.yml` 的 `wunder-server` / `wunder-sandbox` / `extra-mcp` 统一使用同名本地镜像（`wunder-x86`/`wunder-arm`），并设置 `pull_policy: never`，已存在镜像时优先复用，不存在时再自动构建，避免首次启动时 `extra-mcp` 先拉取失败。
- ARM compose 防漂移：`docker-compose-arm.yml` 的 Rust 构建链路显式声明 `build.platforms=linux/arm64`，并在 `wunder-server`/`wunder-sandbox`/`extra-mcp`/`wunder-frontend`/`wunder-nginx` 启动时执行架构自检；若运行时非 arm64 会立即失败并提示重建命令，避免误用旧的 x86 镜像标签。
- 前端入口：管理端调试 UI `http://127.0.0.1:18000`，调试前端 `http://127.0.0.1:18001`（Vite dev server），用户侧前端 `http://127.0.0.1:18002`（Nginx 静态服务）。
- Single-port docker compose mode: expose only `18001` publicly; proxy `/wunder`, `/a2a`, and `/.well-known/agent-card.json` to `wunder-server:18000`; keep `wunder-postgres`/`wunder-weaviate`/`extra-mcp` bound to `127.0.0.1`.
- 鉴权：管理员接口使用 `X-API-Key` 或 `Authorization: Bearer <api_key>`（配置项 `security.api_key`），用户侧接口使用 `/wunder/auth` 颁发的 `Authorization: Bearer <user_token>`；外部系统嵌入接入使用 `security.external_auth_key`（环境变量 `WUNDER_EXTERNAL_AUTH_KEY`）调用 `/wunder/auth/external/*`。当未显式配置 `external_auth_key` 时会自动回退到 `security.api_key`，即默认启用外链鉴权；`/login?token=<team_jwt>&user_id=<id>[&agent_name=<name>]` 当前走 `/wunder/auth/external/token_login` 直换 wunder `access_token`（JWT 校验失败不阻断登录）。当未传 `agent_name`，或名称未命中当前用户可访问的已有智能体时，接口返回默认智能体 `agent_id=__default__` 且 `focus_mode=false`，前端跳转 `/app/chat?section=messages&entry=default`（desktop 为 `/desktop/chat?section=messages&entry=default`）；当 `agent_name` 命中当前用户可访问的已有智能体时，接口返回对应 `agent_id` 与 `focus_mode=true`，前端进入 `/app/embed/chat`（desktop 为 `/desktop/embed/chat`）并隐藏左/中栏聚焦该智能体。
- 用户资料接口：`GET /wunder/auth/me` 会额外返回 `usage_summary`（当前用于用户侧“我的概况”展示累计消耗与工具调用数）；`PATCH /wunder/auth/me` 支持更新 `username/email/unit_id`，并保持返回同一结构；已登录用户如同时提交 `current_password` 与 `new_password`，服务端会先校验当前密码，再更新自己的登录密码。
- 默认管理员账号为 admin/admin，服务启动时自动创建且不可删除，可通过用户管理重置密码。
- 用户端请求可省略 `user_id`，后端从 Token 解析；管理员接口可显式传 `user_id` 以指定目标用户。
- 模型配置新增 `model_type=llm|embedding`，向量知识库依赖 embedding 模型调用 `/v1/embeddings`。
- 用户侧前端默认入口为 `/app/home`（desktop 为 `/desktop/home`）；`/app/home|chat|user-world|workspace|tools|settings|profile|channels|cron` 统一复用 Messenger 壳。嵌入聊天路由为 `/app/embed/chat`（desktop `/desktop/embed/chat`，demo `/demo/embed/chat`），用于外链接入时聚焦单个已命中的智能体，并隐藏左/中栏；普通默认外链入口仍进入 `/app/chat`（desktop `/desktop/chat`）。外链详情路由为 `/app/external/:linkId`（demo 为 `/demo/external/:linkId`）。External links are managed via `/wunder/admin/external_links` and delivered by `/wunder/external_links` after org-level filtering; production frontend port is 18002, development port is 18001。
- 当使用 API Key/管理员 Token 访问 `/wunder`、`/wunder/chat`、`/wunder/workspace`、`/wunder/user_tools` 时，`user_id` 允许为“虚拟用户”，无需在 `user_accounts` 注册，仅用于线程/工作区/工具隔离。
- 渠道 webhook 入站默认采用“快速 ACK + 后台队列分发”：`/wunder/channel/*/webhook` 完成验签与标准化后立即入队，模型/工具链路在后台执行；当入站队列短时拥塞时接口返回 `503` 以触发渠道侧重试。
- QQ Bot 渠道支持两种入站模式：`/wunder/channel/qqbot/webhook` 回调模式，以及账号级长连接模式（`qqbot.long_connection_enabled=true`，默认开启）；凭证可使用 `qqbot.app_id + qqbot.client_secret` 或 `qqbot.token=appId:clientSecret`；未显式配置 `qqbot.intents` 时长连接会按 `full -> group+channel -> channel-only` 自动降级重试，并写入渠道运行日志事件。
- 渠道附件出站（2026-03-18）：Feishu 支持上传后发送原生 `image/file` 消息；XMPP 出站会附带 `jabber:x:oob` 与 `urn:xmpp:reference:0` 节点；QQBot 在 group/user 目标支持通过 `/v2/*/files` 发送富媒体 URL（image/video/audio）。
- 渠道附件入站（2026-03-18）：QQBot URL 附件会在入站阶段下载到会话作用域工作区；Feishu/XMPP 保持既有落盘能力。
- 渠道链接改写（2026-03-18）：`channel_outbox` 不仅会改写正文中的 `/workspaces/...`，也会改写 `attachments[].url` 中的工作区路径为 `/wunder/temp_dir/download`。
- 工作区容器约定：用户私有容器固定为 `container_id=0`，智能体容器范围为 `1~10`；`/wunder/workspace*` 全部接口（含 upload）支持显式 `container_id`，且优先级高于 `agent_id` 推导。
- Desktop 本地模式下，这些容器默认映射到本地持久目录，不执行“24 小时自动清理”策略；用户文件需显式删除。内置文件工具在本地模式下还支持直接访问本机绝对路径，不再强制限制在工作区内。
- Desktop 本地模式固定优先使用安装包附带的 Python 运行时，不再通过 `/wunder/desktop/settings` 配置自定义解释器，也不再提供 `/wunder/desktop/python/interpreters` 本机探测接口；`GET /wunder/desktop/fs/list` 仍保留用于本地目录浏览等通用场景。
- Desktop 引导接口 `GET /config.json` 与 `GET /wunder/desktop/bootstrap` 现补充 `runtime_profile` 与 `runtime_capabilities`：前者用于标识 `desktop_embedded` / 其他运行形态，后者用于下发 `embedded_mode/thread_runtime_active/mission_runtime_active/cron_active/channels_enabled/channel_outbox_worker_enabled/lan_overlay_supported` 等能力位，供前端按实际运行能力启用订阅、恢复与降级策略。
- 控制平面实时状态已收敛到 `state.control.presence`：当前主要负责连接在线态与最近活跃时间，为在线列表与连接恢复提供基础数据。
- Desktop 本地模式默认开启 `channels.outbox.worker_enabled=true`，保障 `channel_tool.send_message` 入队后自动投递，无需管理员侧手工启用出站 worker。
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
- 在线态来源：联系人列表中的 `online/last_seen_at` 由 `connection presence` 提供。
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
- 队列回放：`queue_enter/queue_start/queue_finish/queue_fail` 现已进入 `stream_events` 持久化流，`watch/resume`、刷新重连和 SSE/WS 补偿都可回放。
- WS 接管语义：客户端收到 `queued` 后应立即切换到 `watch`，而不是继续等待原始 `start` 请求流结束。
- 排队活跃态：`watch/resume` 会把同 `session` 下的 `pending/retry/running` 队列任务视为活跃流状态，排队期仍会维持恢复链路与心跳。
- 慢客户端恢复：当 WS 出站队列接近满载时，服务端会发送 `slow_client(reason=queue_full_resume_required)`，调用方应改走 `resume/watch` 补齐，而不是假设增量仍会持续直推。

## 4.x 子智能体控制补充（2026-03-26）

- `subagent_control` 现支持 `action=batch_spawn|status|wait|interrupt|close|resume`，与既有 `list|history|send|spawn` 共用同一入口。
- `batch_spawn` 支持一次派发多个子智能体任务，返回稳定 `dispatch_id`；支持 `strategy=parallel_all|first_success|review_then_merge`，其中 `first_success` 用于对齐 Codex 式的“首个成功即先返回”协作收敛语义，并会在同次等待收敛时默认对未完成兄弟分支执行 `remainingAction=interrupt`。
- `wait` 现支持 `waitMode=all|any|first_success`：`all` 等待全部目标结束，`any` 在首个目标进入终态后返回，`first_success` 在首个成功出现后返回，否则继续等到全部结束或超时。
- `batch_spawn/wait` 现支持 `remainingAction=keep|interrupt|close`：用于在 `first_success/any` 这类提前收敛场景下处理尚未结束的兄弟分支；`wait` 默认 `keep`，`batch_spawn.strategy=first_success` 默认 `interrupt`。
- 新增一级编排工具 `会话让出`（英文别名 `sessions_yield`/`yield`）：用于在成功派发后台子智能体后显式结束当前轮次，并等待子智能体回流结果自动唤醒父线程继续。
- `status/wait` 支持按 `runId/runIds/sessionId/sessionIds/dispatchId/parentId` 查询或等待；未显式传目标时，`status` 默认查询当前会话下最近子会话运行态。
- `interrupt` 基于 monitor 对目标子会话发起取消；`close/resume` 直接切换子会话 `status=closed|active`，并可通过 `cascade=true` 递归作用到后代子会话。
- 子智能体批量调度的运行账本统一落在 `session_runs`，新增元数据字段 `dispatch_id/run_kind/requested_by/metadata`；其中 `metadata` 当前包含 `controller_session_id/parent_turn_ref/depth/role/control_scope`，批量任务还会补充 `dispatch_index/dispatch_size/dispatch_label/strategy/completion_mode/remaining_action`，便于批次级聚合、追踪与恢复。
- `status/wait` 的结果会额外返回 `completion_mode/completion_reached/completed_reason/selected_items`；运行快照中新增 `agent_state.status/message`；批次结果会补充 `winner_item/remaining_action/remaining_action_applied/settled_items`，用于对齐 Codex 协作线程的 winner 选择与剩余分支处置表达。
- `status/wait`、会话级 `subagents` 列表以及聊天消息里的 `messages[].subagents[]` 会同步返回 `metadata/controller_session_id/depth/role/control_scope/spawn_mode` 等结构化字段，前端可以直接渲染子智能体工作区，不再依赖聊天文本推断谱系。
- 流式事件新增 `subagent_dispatch_start/subagent_dispatch_item_update/subagent_dispatch_finish/subagent_status/subagent_interrupt/subagent_close/subagent_resume/subagent_announce`，其中批次开始/结束事件会携带 `strategy/completion_mode/remaining_action` 供前端工作流展示。
- Codex 风格父子轮次语义：父智能体在成功派发子智能体后不必阻塞等待；父轮可以先发出 `turn_terminal` 并结束，本次对话在用户视角应视为“已结束”，子智能体继续在后台运行。
- 当某个 `dispatch_id` 达到 `completion_mode` 收敛条件，或全部子任务完成后，系统会向父会话追加一条隐藏内部观察消息并自动唤醒父线程继续推理；该观察消息会参与轮次对齐，但在聊天历史里会标记 `hiddenInternal=true`，前端默认不渲染正文。
- 新增会话级子智能体接口：
  - `GET /wunder/chat/sessions/{session_id}/subagents`：返回当前父会话可见的子智能体运行项列表，支持 `limit`
  - `POST /wunder/chat/sessions/{session_id}/subagents/control`：支持 `action=interrupt|terminate|close`，并可通过 `sessionIds[]` 或 `dispatchId` 批量控制当前父会话下的子智能体
- 忙时返回：当 `agent_queue.enabled=false` 且显式指定 `session_id` 正在运行/取消中时，会返回 429（`detail.code=USER_BUSY`）。
- 说明：未传 `session_id` 且主会话正忙时，会自动分叉独立会话继续处理，并返回新的 `session_id`（不覆盖主会话）。
- 说明：问询面板进入 `waiting` 后，用户选择路线会被当作正常请求立即继续处理，不会被判定为“会话繁忙”进入队列。
- 约束：全局并发上限由 `server.max_active_sessions` 控制，超过上限的请求会排队等待。
- 约束：同一轮同类工具连续失败达到 `server.tool_failure_guard_threshold`（默认 5）会触发 `tool_failure_guard` 并停止自动重试；若同一工具命中同一个明确的不可重试错误，默认会在第 3 次相同失败后提前触发保护，避免模型持续硬撞同一错误。
- 说明：管理员会话跳过上述限制（会话锁/额度/并发上限）。
- 说明：当 `tool_names` 显式包含 `a2ui` 时，系统会剔除“最终回复”工具并改为输出 A2UI 消息；SSE 将追加 `a2ui` 事件，非流式响应会携带 `uid`/`a2ui` 字段。
- 流式异常事件：`error` 事件现在会统一附带 `error_meta`（`category/severity/retryable/retry_after_ms/source_stage/recovery_action`），便于前端与调用方区分“可重试失败”和“需人工修正失败”。
- 流式终结事件：新增 `turn_terminal`，作为每轮执行的唯一终结语义，`status` 取值包括 `completed/failed/cancelled/rejected`；`final.stop_reason` 现可能为 `yield`，表示模型主动调用 `sessions_yield` 结束本轮并转入后台子智能体续跑；调用方不应再仅靠 `final/error` 自行猜测一轮是否已结束。
- 审批闭环事件：新增 `approval_resolved`，表示待审批请求已进入终态；`approval_result` 保持兼容，但新接入方应优先消费 `approval_resolved`。
- 工具工作流关联语义：`tool_call/tool_output_delta/tool_result/approval_request/approval_result` 现在会尽量附带稳定的 `tool_call_id`；当上游没有原生 call id 时，服务端会补发合成 id，便于前端将命令输出、审批等待与最终结果持续合并到同一张工作流卡片。
- `execute_command` 第一阶段实时协议已落地：`tool_output_delta` 与每条命令结果会补充 `command_session_id/command_index`，用于把一次工具调用内的多条子命令拆成独立工作流条目。
- 新增命令会话生命周期事件：`command_session_start/command_session_status/command_session_exit/command_session_summary`。当前阶段只持久化生命周期与摘要事件，不向客户端额外广播高频 `command_session_delta`，避免在旧前端仍消费 `tool_output_delta` 时造成双倍热路径流量。
- 线程运行态事件：新增 `thread_status`，用于同步 loaded runtime 状态机；`status` 取值包括 `running/waiting_approval/waiting_user_input/idle/not_loaded/system_error`，并附带 `session_id/thread_id/subscriber_count/loaded/active_turn_id`。
- 会话事件摘要接口：`GET /wunder/chat/sessions/{session_id}/events` 现额外返回 `data.runtime` 快照（包含 `thread_status/loaded/active_turn_id/turn.pending_approval_count/turn.waiting_for_user_input` 等字段）；`data.running` 也会覆盖等待审批、等待用户输入等活跃态，便于刷新后继续保持实时等待视图。
- 会话级实时订阅支持 `cancel`、连接关闭和任务自然结束后的幂等清理，避免断连后残留状态。
- 命令会话摘要现并入 `GET /wunder/chat/sessions/{session_id}/events`：返回 `data.command_sessions[]`，每项为当前会话内仍保留在 Broker 中的命令会话快照，包含 `command_session_id/status/seq/started_at/updated_at/ended_at/exit_code/stdout_tail/stderr_tail/pty_tail/*_dropped_bytes` 等字段，用于前端刷新后直接恢复工作流里的终端预览。
- 新增命令会话回放接口：
  - `GET /wunder/chat/sessions/{session_id}/command-sessions`：返回当前会话可见的命令会话快照列表。
  - `GET /wunder/chat/sessions/{session_id}/command-sessions/{command_session_id}`：返回单个命令会话快照，按 `user_id + session_id + command_session_id` 做作用域校验。
- 线程卸载事件：新增 `thread_closed`，表示当前 loaded runtime 已卸载；当最后一个流式订阅者离开且该线程没有 active turn 时会发出，payload 附带 `last_status` 便于前端做状态收尾。
- `context_usage` 事件在模型配置存在有效上下文上限时会额外附带 `max_context`，用于前端展示“上下文占用/上限”。
- 审批作用域：待审批请求现在由共享注册表统一管理，但 `chat/ws` 的 `approval` 与 `cancel` 只会消费 `source=chat_ws` 的待审批项，不会误清理渠道侧审批；渠道内回复 `1/2/3` 也只会作用于 `source=channel` 的审批上下文。
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
- `user_id`：字符串，可选，用户唯一标识（传入后返回用户自建工具；共享工具字段兼容保留但固定为空）
- 返回（JSON）：
  - `builtin_tools`：内置工具列表（name/description/input_schema）
  - `mcp_tools`：MCP 工具列表（name/description/input_schema）
  - `a2a_tools`：A2A 服务工具列表（name/description/input_schema）
  - `skills`：技能列表（name/description/input_schema）
  - `knowledge_tools`：知识库工具列表（字面/向量，name/description/input_schema）
  - `user_tools`：自建工具列表（name/description/input_schema）
  - `user_mcp_tools`：当前用户自建 MCP 工具列表（仅传入 `user_id` 时返回）
  - `user_skills`：当前用户自建技能列表（仅传入 `user_id` 时返回）
  - `user_knowledge_tools`：当前用户自建知识库工具列表（仅传入 `user_id` 时返回）
  - `shared_tools`：共享工具列表（兼容字段，当前固定为空）
  - `shared_tools_selected`：共享工具勾选列表（兼容字段，当前固定为空或 `null`）
  - `items`：统一能力目录列表；字段包括 `id/name/runtime_name/display_name/description/input_schema/group/source/kind/owner_id/available/selected`
- 说明：
  - 用户自建工具名称统一为 `user_id@工具名`（MCP 为 `user_id@server@tool`）。
  - `items[]` 为新的统一能力输出；旧的分组字段继续保留，便于前端与旧调用方平滑迁移。
  - 共享工具能力已停用；服务端不再扫描其他用户工作区，也不再把共享字段纳入运行时编排。
  - 知识库工具入参支持 `query` 或 `keywords` 列表（二选一），`limit` 可选；向量知识库会按关键词逐一检索并在结果中返回 `queries` 分组（多关键词时 `documents` 追加 `keyword`）。
- 内置工具名称同时提供英文别名（如 `read_file`、`write_file`），可用于接口选择与工具调用。
- `列出文件`（`list_files`）新增分页参数：`cursor`（字符串游标）/`offset`（兼容数值偏移）与 `limit`（默认 `500`，最大 `500`）；返回结果补充 `cursor/offset/limit/returned/has_more/next_cursor/next_offset`。推荐优先按页续取，避免一次性回放超长目录导致上下文膨胀。
- `搜索内容`（`search_content`）支持双引擎：`engine=auto|rg|rust`（`auto` 优先 `rg`，失败自动回退 `rust`），并兼容一批更接近 `rg` 心智模型的别名：`pattern`、`glob -> file_pattern`、`type`（常见语言/后缀快捷过滤）、`context/-C`、`-A/-B`、`ignore_case/-i`、`fixed_strings/-F`、`max_count/head_limit -> max_matches`。默认语义更新为：`query` 省略 `query_mode` 时按 `literal` 处理，`pattern` 省略 `query_mode`/`-F` 时按 `regex` 处理；对 `query` 的多词精确短语在无结果时，工具可能自动回退为按词检索，以更贴近模型的自然查询习惯。对于单个超长文件的高密度命中，结果会优先分散暴露不同区段的代表命中，避免预览长期偏在文件前部。该工具只搜索本地工作区文本文件，不会访问网页；返回结果会补充 `resolved_path/scope/scope_note`，零命中时 `summary.next_hint` 会明确提示“本地范围为空或需先 list_files”。
- `读取文件`（`read_file`）仅用于代码/配置/日志/Markdown 等本地纯文本的定点读取，不应用于图片、PDF、Office 文档、压缩包或其他二进制文件，也不应把它当作“整篇吞长文档”的工具。该工具支持 `mode=slice|indentation`：`indentation` 模式可传 `indentation.anchor_line/max_levels/include_siblings/include_header/max_lines`，用于按缩进树读取代码块并降低上下文占用。`slice` 模式的 `start_line/end_line` 采用包含式区间；当模型传入 `start_line=0` 时会自动归一化到首行，但若 `start_line > end_line` 会直接返回 `TOOL_READ_INVALID_RANGE`，避免把反向区间静默吞成假成功。同文件相邻/重叠切片在同一次调用内会合并展示，以减少重复输出与外层截断。另兼容 Codex 常见的 `file_path + offset/limit` 读取窗口写法，并在 `meta.files[]` 中补充 `resolved_path/requested_ranges/effective_ranges/range_args_normalized/used_default_range/request_satisfied` 方便前端与模型判断原始请求、实际生效范围及本次请求是否真正满足。
- `执行命令`（`execute_command`）在本机与 sandbox 返回统一输出护栏元信息：`output_meta`（每条命令）与 `meta.output_guard`（聚合）；若 `content` 为纯补丁正文（`*** Begin Patch ... *** End Patch`），会自动路由到 `应用补丁` 并在结果追加 `intercepted_from=execute_command`。
- 工具结果默认允许约 `20000` 字符级别内容进入 `tool_result`/observation（管理员会话同样生效）；若仍因上下文预算被裁剪，系统会在顶层直接返回 `truncated/observation_output_chars/continuation_required/continuation_hint`（不再放入 `meta`）；数据体中可能出现 `data.truncated/original_chars/preview`、表格级 `rows_sampled/rows_omitted`，或数组级 `{"__truncated":true,"omitted_items":N}` 标记，表示当前结果为片段/样本而非全量。
- `执行命令` 支持预算与预演参数：`dry_run`、`time_budget_ms`、`output_budget_bytes`、`max_commands`（也可放入 `budget` 对象）；`dry_run=true` 时仅返回执行计划与预算，不落地执行。
- `写入文件` 与 `应用补丁` 支持 `dry_run` 预演：返回目标文件与变更摘要，不写磁盘。
- `应用补丁` 的 `input` 现支持多层 JSON 包裹自动解包（如 `{"input":"{\"input\":\"*** Begin Patch ... *** End Patch\"}"}`），降低模型重复封装导致的格式失败。
- 当 `应用补丁` 返回 `PATCH_CONTEXT_NOT_FOUND` 时，`error_meta.hint` 会包含“期望旧片段 + 邻近源码 + 最相似窗口差异示例”，便于模型按上下文重新生成补丁。
- `搜索内容` 返回保留兼容字段 `matches`，同时提供结构化 `hits`、`matched_files/matched_file_count/returned_match_count`、`summary` 与 `meta.search`。其中 `summary` 会给出实际采用的策略、顶部相关文件、命中词、`focus_points` 和下一步提示；`meta.search` 额外包含 `query_source`、`query_mode_inferred`、`strategy`、`attempts_tried`、`requested_engine/resolved_engine/rg_program/fallback/elapsed_ms/timeout_hit` 等信息，便于前端与调度层做可观测优化。
- `搜索内容` 支持预算与预演参数：`dry_run`、`time_budget_ms`、`output_budget_bytes`（也可放入 `budget`，并支持 `budget.max_files/max_matches/max_candidates`）；超预算时会在 `meta.search.output_budget_hit` 标记结果裁剪。
- `读取文件` 支持预算与预演参数：`dry_run`、`time_budget_ms`、`output_budget_bytes`、`max_files`（也可放入 `budget`）；结果在 `meta.read` 返回 `timeout_hit/output_budget_hit/budget_file_limit_hit`。当本次只返回了默认大窗口前缀、文件安全截断前缀，或读取结果在外层继续可细化续取时，数据体会显式补 `continuation_required/continuation_hint`，提示模型应先 `search_content` 定位标题或改读更窄的行范围，而不是反复整篇重读。
- 基础工具失败结果统一补充 `error_meta`：`code/hint/retryable/retry_after_ms`，并保证同时落入 `data.error_meta`，便于前端、结果归一化和重试治理统一按错误码做自动恢复。
- 外层工具超时不再只返回笼统字符串；`tool_result` 会补充 `data.failure_summary/error_detail_head/next_step_hint/timeout_s/timeout_ms` 与 `error_meta.code=TOOL_TIMEOUT`，前端工作流可直接显示失败原因与下一步建议。
- 新增内置工具 `计划面板`（英文别名 `update_plan`），用于更新计划看板并触发 `plan_update` 事件。
- 新增内置工具 `问询面板`（英文别名 `question_panel`/`ask_panel`），用于提供多条路线选择并触发 `question_panel` 事件。
- 新增内置工具 `技能调用`（英文别名 `skill_call`/`skill_get`），传入技能名返回完整 SKILL.md 与技能目录结构。
  - 技能文档内建议使用占位符 `{{SKILL_ROOT}}` 引用技能资源（脚本/示例/工作流文件等）。
  - `skill_call` 返回时会将 `skill_md` 中的 `{{SKILL_ROOT}}` 自动替换为本次可见的技能根目录绝对路径（同返回字段 `root`）。
  - `skill_call` 结果不再走通用长度裁剪，避免模型因拿不到完整技能正文而反复回读同一个 `SKILL.md`。
- `读取文件` 的切片读取结果会在 `meta.files[]` 里补充 `hit_eof/range_reaches_eof`，帮助模型判断当前分段是否已触达文件末尾，避免继续请求越界范围；若同文件一次请求了多个离散切片，正文里会增加 `[lines a-b]` 小标题以保持范围边界清晰。
- 新增内置工具 `子智能体控制`（英文别名 `subagent_control`），通过 `action=list|history|send|spawn|batch_spawn|status|wait|interrupt|close|resume` 统一完成子会话派生、批量调度、状态聚合与生命周期控制。
- 新增内置工具 `会话让出`（英文别名 `sessions_yield`/`yield`），用于在完成子智能体派发后主动结束当前轮次，向用户返回一句简短提示，并等待后台子智能体完成后自动唤醒父会话继续。
- 新增内置工具 `会话线程控制`（英文别名 `thread_control`/`session_thread`），通过 `action=list|info|create|switch|back|update_title|archive|restore|set_main` 控制当前用户的线程树，并可触发 `thread_control` 工作流事件驱动前端同步切换线程。
- 新增内置工具 `智能体蜂群`（英文别名 `agent_swarm`/`swarm_control`），通过 `action=list|status|send|history|spawn|batch_send|wait` 管理当前用户“当前智能体以外”的其他智能体。
- `智能体蜂群` 的 `send`/`batch_send` 在未显式传入 `sessionKey` 时默认新建线程，避免工蜂沿用旧上下文；仅在显式指定 `sessionKey` 时复用已有线程。
- `智能体蜂群` 新增 `wait` 动作：可直接等待 `run_ids` 结果并返回聚合状态，避免母蜂反复轮询 `status`。
- 多工蜂协作推荐：先 `batch_send` 一次并发派发，再 `wait` 统一收敛。
- `智能体蜂群` 入参语义增强（便于模型主动调用）：`send`/`spawn` 支持 `agentId` 或 `agentName/name` 直达目标；`send` 需 `message` 且 `agentId/agentName/name/sessionKey` 四选一，`spawn` 需 `task` 且 `agentId/agentName/name` 三选一，`history` 需 `sessionKey`，`wait` 需 `runIds`，`batch_send` 需 `tasks[]`（每项需 `message` 且 `agentId/agentName/name/sessionKey` 四选一）。
- `智能体蜂群` 的动态提示仅注入到工具描述本身，展示“工蜂名称 + 一句话描述”；已冻结线程的 system prompt 不会因工蜂变化而改写。
- 推荐最短调用路径：`list -> batch_send -> wait -> history/status`（单目标用 `send` 替代 `batch_send`）。
- `子智能体控制` 的 `send` 支持 `timeoutSeconds` 等待回复，`spawn` 支持 `runTimeoutSeconds` 等待完成并返回 `reply/elapsed_s`；`batch_spawn` 会返回稳定 `dispatch_id` 并把父轮次引用写入每个子任务，便于后续在消息气泡内聚合展示。
- 推荐的 Codex 风格子智能体调用路径更新为：`subagent_control.spawn/batch_spawn -> sessions_yield -> 子智能体自动回流唤醒 -> status/wait(按需)`；其中 `sessions_yield` 是显式“本轮先结束”的一级原语。
- `会话线程控制` 的 `create/switch/back/set_main` 可同时更新主线程绑定；当工具通过流式通道返回 `thread_control` 事件时，用户前端会先合并会话摘要，再按 payload 决定是否切换到目标线程。
- 新增内置工具 `节点调用`（英文别名 `node.invoke`/`node_invoke`），通过 `action=list|invoke` 统一完成节点发现与节点调用。
- 新增内置工具 `用户世界工具`（英文别名 `user_world`），通过 `action=list_users|send_message` 获取用户列表或发送私信（消息会在用户世界页面可见）。
- 新增内置工具 `渠道工具`（英文别名 `channel_tool`），通过 `action=list_contacts|send_message` 查询渠道可联系对象并向指定渠道对象发送消息（支持工作区文件引用转下载链接后发送）。
- `渠道工具.list_contacts` 默认融合会话历史与 XMPP roster（若可用），返回 `source=session_history|roster|session_history+roster`；可传 `refresh=true` 强制刷新 roster 缓存。
- `渠道工具.send_message` 参数已简化：不再强制 `channel/account_id/to` 同时必填；可直接传 `text`（或 `content`/`attachments`）并由系统从会话/默认账号自动补全。`list_contacts` 返回 `contact` 对象，可直接回传给 `send_message`。
- `渠道工具.send_message` 附件投递能力（2026-03-18）：Feishu/XMPP/QQBot 优先走渠道原生附件链路；若目标渠道不支持对应类型则自动回退为文本链接，不阻断投递。
- 测试开放态（2026-03-11）：`channel_tool` 默认放开账号归属限制，`list_contacts` 可读取当前系统内所有已配置渠道账号；渠道请求默认覆盖 `security.approval_mode=full_auto` 与 `security.exec_policy_mode=allow`，不再进入渠道审批提示链路。
- 浏览器工具重构（2026-03-27）：内置工具 `浏览器`（英文别名 `browser`）升级为浏览器运行时入口，支持 `status/profiles/start/stop/tabs/open/focus/close/navigate/snapshot/act/screenshot/read_page`；保留 `browser_navigate/browser_click/browser_type/browser_screenshot/browser_read_page/browser_close` 旧别名兼容。浏览器工具对模型的可见性由 `tools.browser.enabled` 控制，浏览器运行时由顶层 `browser.*` 配置控制；非 desktop 模式下无需再把 `浏览器` 写进 `tools.builtin.enabled`，`desktop + tools.browser.enabled` 仍兼容 legacy 模式。
- 新增浏览器控制接口（2026-03-27）：`/wunder/browser/health`、`/wunder/browser/status`、`/wunder/browser/profiles`、`/wunder/browser/session/start`、`/wunder/browser/session/stop`、`/wunder/browser/tabs`、`/wunder/browser/tabs/open`、`/wunder/browser/tabs/focus`、`/wunder/browser/tabs/close`、`/wunder/browser/navigate`、`/wunder/browser/snapshot`、`/wunder/browser/act`、`/wunder/browser/screenshot`、`/wunder/browser/read_page`。
- 内置工具 `网页抓取`（英文别名 `web_fetch`）支持 `extract_mode=markdown|text` 与 `max_chars`；直接通过 HTTP 抓取网页并输出低噪声正文，不用于本地文件或关键词搜索，并会对明显的前端壳页/验证页返回结构化失败或自动切换浏览器兜底。
- `网页抓取` 默认执行正文清洗与去噪，移除导航、页脚、广告、评论等低价值片段；同时内置重定向复校验、响应体大小限制与短 TTL 缓存。私网/内网目标默认拦截，但现可通过 `tools.web.fetch.allow_private_network=true` 全量放开，或用 `tools.web.fetch.hostname_allowlist` 按主机名/IP 精确放行。
- `网页抓取` 的失败结果现结构化暴露 `phase`（如 `validation/dns_lookup/request/response_body/extract`）、`failure_summary`、`next_step_hint` 与 `error_meta`；浏览器桥启动失败也会在 ready 前返回结构化 JSON，便于工作流区域直接展示真实故障原因（例如缺少 Playwright 浏览器二进制）。
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

### 4.1.2A 智能体应用与模型选择（`/wunder/agents`）

#### `GET /wunder/agents/models`

- 方法：`GET`
- 入参（Query，可选）：`user_id`
- 返回（JSON）：
  - `data.items`：可选模型配置名列表（仅 `model_type=llm`）
  - `data.default_model_name`：当前默认模型配置名（可能为 `null`）
- 说明：
  - 返回的模型名为 **模型配置键**，不是上游 provider 的原始模型字符串。
  - 用户侧“智能体设置/新建智能体/工蜂卡导入导出”应使用该列表作为可选项。

#### `GET /wunder/agents`

- 方法：`GET`
- 入参（Query，可选）：`user_id`、`hive_id`
- 返回（JSON）：`data.items[]`（智能体列表）
- 预设补齐：仅在用户首次访问该列表时执行一次默认预设智能体补齐；后续用户对预设实例的重命名或删除不会在列表读取时自动生成重复副本，如需补回由管理员预设同步触发。
- 与模型选择相关字段：
  - `configured_model_name`：该智能体显式配置的模型；为空表示跟随默认模型
  - `model_name`：当前生效模型（优先取 `configured_model_name`，否则回退到默认模型）
- 与能力模型相关字段：
  - `ability_items`：结构化能力列表，字段包括 `id/name/runtime_name/display_name/description/input_schema/group/source/kind/owner_id/available/selected`
  - `abilities.items`：与 `ability_items` 等价的嵌套兼容字段
  - `tool_names`：当前运行时启用的能力名列表（兼容字段）
  - `declared_tool_names` / `declared_skill_names`：仅表示 worker-card 导入时声明的工具/技能依赖；普通智能体不要求写入

#### `POST /wunder/agents`

- 方法：`POST`
- 入参（Query）：`user_id`
- 入参（JSON）：
  - `name`：智能体名称（必填）
  - `model_name`：模型配置名（可选，支持 `modelName`/`model_name`；空值表示使用默认模型）
  - `ability_items`：结构化能力列表（可选）
  - `abilities.items`：结构化能力列表的嵌套写法（可选，等价于 `ability_items`）
  - `declared_tool_names`：工蜂卡声明的非技能工具依赖（可选）
  - `declared_skill_names`：工蜂卡声明的技能依赖（可选）
  - 其余字段同现有智能体创建接口（如 `description/system_prompt/tool_names/...`）
- 说明：
  - 工蜂卡导入/导出时，技能声明应落在 `declared_skill_names`，不要混入 `declared_tool_names`
  - 当提交 `ability_items`/`abilities.items` 时，后端会把它作为结构化能力主数据持久化；`tool_names` 继续作为运行时兼容字段保留

#### `PUT /wunder/agents/{agent_id}`

- 方法：`PUT`
- 入参（Query）：`user_id`
- 入参（JSON）：
  - `model_name`：模型配置名（可选，支持 `modelName`/`model_name`；空值表示清除显式配置并回退默认模型）
  - `ability_items` / `abilities.items`：结构化能力列表（可选，增量更新时可单独提交）
  - 其余字段按需增量更新
- 说明：预设智能体实例使用稳定 `preset_binding` 跟踪模板关系；用户侧重命名不会丢失绑定，也不会触发同名预设副本再次自动补种。
- 工蜂卡相关说明：
  - 更新时可同时提交 `declared_tool_names` 与 `declared_skill_names`
  - 技能依赖应写入 `declared_skill_names`

#### `GET /wunder/agents/{agent_id}/runtime-records`

- 方法：`GET`
- 入参（Query，可选）：`user_id`、`days`（1~30，默认 14）、`date`（`YYYY-MM-DD`，热力图选中日期）
- 返回（JSON）：
  - `data.summary.runtime_seconds`：统计窗口内总运行时长
  - `data.summary.billed_tokens`：兼容字段，等价于累计消耗
  - `data.summary.consumed_tokens`：推荐字段，表示统计窗口内累计消耗
  - `data.summary.quota_consumed`：统计窗口内额度消耗
  - `data.summary.tool_calls`：统计窗口内工具调用次数
  - `data.daily[]`：按日拆分，字段包括 `date/runtime_seconds/billed_tokens/consumed_tokens/quota_consumed/tool_calls`
  - `data.heatmap`：工具调用热力图（`date/max_calls/items[]`）
- 说明：
  - `consumed_tokens` 按各次请求的 `round_usage.total_tokens` 累加得到。
  - `billed_tokens` 为历史兼容字段，新接入优先使用 `consumed_tokens`。

#### `GET /wunder/admin/preset_agents`

- 方法：`GET`
- 返回：`data.items[]`
- 说明：
  - 列表会额外补充模板用户 `preset_template` 的默认智能体，返回项携带 `preset_id="__default__"` 与 `is_default_agent=true`。
  - 普通预设返回的 `preset_id` 是内部稳定标识，用于绑定、版本递增与存量同步；管理端界面不应将其作为展示名或用户输入项。
  - 该默认智能体项不会写入普通 `user_agents.presets`；管理端可直接编辑其默认模板，并可复用同一同步接口将默认智能体设置同步到存量用户。

#### `POST /wunder/admin/preset_agents`

- 方法：`POST`
- 入参（JSON）：`items[]`
- 预设项新增字段：
  - `model_name`：预设智能体默认模型配置名（可选，支持 `modelName`/`model_name`）
- 说明：
  - 管理端预设保存后，模板用户同名智能体会同步该 `model_name`。
  - 新注册用户或存量同步时，若该字段非空，会将该模型配置下发到用户智能体。
  - 若提交项中包含 `preset_id="__default__"` 的默认智能体特殊项，服务端会忽略该项，避免将默认智能体误写成普通预设。
  - 预设工蜂卡目录与管理员导出文件名默认只使用名称，`preset_id` 仍只作为后端模板绑定键；卡片协议中对应的内部稳定标识改为 `metadata.agent_id`，值继续统一使用 `preset_<stable-hash>` 形态，不作为用户可见文件名前缀；预设版本与启停状态写入卡片顶层 `preset.{revision,status}`，不再放在 `extensions`。

#### `POST /wunder/admin/preset_agents/sync`

- 方法：`POST`
- 入参（JSON）：
  - `preset_id`：预设 ID；当传入 `__default__` 时，表示同步模板用户默认智能体配置。
  - `mode`：`safe` / `force`
  - `dry_run`：是否仅预演
- 说明：
  - `preset_id="__default__"` 仅同步默认智能体的设置字段（名称、描述、提示词、工具、问题、审批模式、状态、工作目录与图标）。
  - 默认智能体同步同样支持 `safe` / `force`：`safe` 只覆盖仍跟随模板的字段，`force` 强制覆盖模板管理字段。

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
- 说明：`shared_tools` 为兼容字段，当前保存与返回时固定为空数组。

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
  - `shared`：已共享技能名列表（兼容字段，当前固定为空）
  - `skills`：技能列表（name/description/path/input_schema/enabled/shared/builtin/source/readonly）
    - `source`：`builtin` 或 `custom`
    - `builtin=true`/`readonly=true` 表示内置技能（只读）
- `POST` 入参（JSON）：
  - `user_id`：用户唯一标识
  - `enabled`：启用技能名列表
  - `shared`：共享技能名列表（兼容字段，当前会被服务端清空）
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
- 说明：上传内容写入自定义技能目录（`source=custom`），不会覆盖内置 `config/skills/` 源码目录。
- 说明：上传目录若与内置技能目录冲突会返回 `403`（避免覆盖内置技能）。
- 说明：压缩包必须以“技能目录”为顶层，例如 `我的技能/SKILL.md`；不允许直接把 `SKILL.md`、脚本或其他文件放在压缩包根目录，否则会返回 `400`。

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
- 说明：`base_type` 为空默认字面知识库；`base_type=vector` 时必须指定 `embedding_model`，root 自动指向 `config/data/vector_knowledge/users/<user_id>/<base>` 作为逻辑标识，向量文档与切片元数据存储在数据库中。

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
  - `reasoning`：字面知识库测试时模型返回的思考过程文本
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
  - `shared_tools`：共享工具列表（兼容字段，当前固定为空）
  - `shared_tools_selected`：共享工具勾选列表（兼容字段，当前固定为空数组）
  - `items`：统一能力目录列表；字段包括 `id/name/runtime_name/display_name/description/input_schema/group/source/kind/owner_id/available/selected`
- 说明：返回的是当前用户实际可用工具（已按等级与用户自身配置过滤）。
- 说明：知识库工具入参支持 `query` 或 `keywords` 列表（二选一），`limit` 可选。
- 说明：`items[]` 与旧分组字段同时返回；推荐新前端与新工具目录逻辑优先消费 `items[]`。

### 4.1.2.20 `/wunder/user_tools/catalog`

- 方法：`GET`
- 返回（JSON）：
  - 兼容保留 `/wunder/user_tools/tools` 的扁平字段：`builtin_tools/mcp_tools/a2a_tools/skills/knowledge_tools/user_tools/shared_tools/shared_tools_selected`
  - `admin_builtin_tools`：管理员开放给当前用户的内置工具
  - `admin_mcp_tools`：管理员开放给当前用户的 MCP 工具
  - `admin_a2a_tools`：管理员开放给当前用户的 A2A 工具
  - `admin_skills`：管理员开放给当前用户的技能
  - `admin_knowledge_tools`：管理员开放给当前用户的知识库工具
  - `user_mcp_tools`：当前用户配置的自建 MCP 工具
  - `user_skills`：当前用户配置的自建技能
  - `user_knowledge_tools`：当前用户配置的自建知识库工具
  - `default_agent_tool_names`：默认智能体/预制智能体新建时的默认勾选项
  - `items`：统一能力目录列表；字段包括 `id/name/runtime_name/display_name/description/input_schema/group/source/kind/owner_id/available/selected`
- 说明：用于智能体设置与工具管理页面；`shared_tools/shared_tools_selected` 仅为兼容字段，当前恒为空。
- 说明：管理员开放工具与用户自建工具已拆分为独立区域。服务端/云端模式下管理员开放工具是否可见由管理员配置决定；用户自建 MCP/技能/知识库只要已配置就会进入对应区域，不再依赖用户侧额外“启用”开关。
- 说明：`items[]` 是目录接口的统一能力视图：`group` 用于前端分组展示，`kind` 用于区分工具/技能；旧字段继续保留用于兼容。
- 说明：desktop 本地模式下，`builtin_tools/admin_builtin_tools` 默认返回全部内置工具（按运行能力过滤），不再依赖 `tools.builtin.enabled` 白名单。
- 说明：`default_agent_tool_names` 当前固定收敛为默认画像：`最终回复/定时任务/休眠等待/记忆管理/执行命令/ptc/列出文件/搜索内容/读取文件/技能调用/写入文件/应用补丁`，以及默认技能 `技能创建器`；MCP/知识库默认不勾选。

### 4.1.2.21 `/wunder/user_tools/shared_tools`

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：用户唯一标识（可选）
  - `shared_tools`：共享工具勾选列表（兼容字段，当前会被忽略）
- 返回（JSON）：
  - `user_id`：用户唯一标识
  - `shared_tools`：共享工具勾选列表（当前固定为空数组）

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
- 说明：默认从项目根目录 `config/data/temp_dir/` 目录读取文件并下载；可通过环境变量 `WUNDER_TEMP_DIR_ROOT` 指定根目录。
- 返回：文件流（`Content-Disposition: attachment`）

### 4.1.2.25 `/wunder/temp_dir/upload`

- 方法：`POST`
- 鉴权：无
- 类型：`multipart/form-data`
- 入参：
  - `file` 文件字段（支持多个同名字段）
  - `path` 目标子目录路径（相对 `temp_dir/`，可选）
  - `overwrite` 是否覆盖同名文件（可选，默认 true）
- 说明：默认上传文件到项目根目录 `config/data/temp_dir/`，若设置 `path` 则自动创建目录；可通过环境变量 `WUNDER_TEMP_DIR_ROOT` 指定根目录。
- 返回（JSON）：
  - `ok`：是否成功
  - `files`：上传后的文件名列表

### 4.1.2.26 `/wunder/temp_dir/list`

- 方法：`GET`
- 鉴权：无
- 说明：列出临时目录文件（包含子目录，返回相对路径）；默认根目录为项目根 `config/data/temp_dir/`，可通过环境变量 `WUNDER_TEMP_DIR_ROOT` 指定。
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
- 说明：默认操作项目根目录 `config/data/temp_dir/`；可通过环境变量 `WUNDER_TEMP_DIR_ROOT` 指定根目录。
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
- `GET /wunder/cron/list`：列出当前用户的定时任务（可选 `agent_id`，按智能体作用域过滤）
  - 返回：`data.jobs`（包含 job_id/name/schedule/next_run_at/last_status/consecutive_failures/auto_disabled_reason 等）
- `GET /wunder/cron/status`：查询调度器健康状态与当前用户任务概况
  - 返回：`data.scheduler`（started/enabled/running_jobs/next_run_at/last_tick_at/last_error、`poll_interval_ms`、`max_idle_sleep_ms`、`lease_ttl_ms`、`lease_heartbeat_ms`、`max_concurrent_runs`、`idle_retry_ms`、`max_busy_wait_ms`、`max_consecutive_failures` 等）+ `data.jobs_total/jobs_enabled/jobs_running`；任务项额外返回派生字段 `running/heartbeat_at/lease_expires_at` 用于前端与模型判断当前执行态。
- `GET /wunder/cron/runs?job_id=...&limit=...&agent_id=...`：查询任务运行记录（传入 `agent_id` 时校验任务归属）
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

### 4.1.2.31 `/wunder/channels/runtime_logs`

- 方法：`GET`
- 说明：用户侧渠道运行日志查询接口；用于在渠道设置面板展示长连接告警/重连信息，服务端对重复日志做时间窗口聚合（防洪）。
- 入参（Query）：
  - `user_id`：用户唯一标识（可选）
  - `channel`：渠道过滤（可选）
  - `account_id`：账号过滤（可选）
  - `agent_id`：按智能体过滤（可选，仅返回该智能体绑定账号对应日志）
  - `limit`：返回条数（可选，默认 80，最大 200）
- 返回（JSON）：
  - `data.items`：日志列表（按时间倒序）
    - `id`：日志记录标识
    - `ts`：时间戳（秒）
    - `level`：日志等级（`info/warn/error`）
    - `channel`：渠道名
    - `account_id`：账号 ID（若为空表示渠道级日志）
    - `event`：事件类型（如 `long_connection_failed`）
    - `message`：日志内容
    - `repeat_count`：聚合计数（同类日志在窗口内重复次数）
  - `data.total`：本次返回条数
  - `data.status`：运行状态摘要
    - `collector_alive`：日志采集器是否存活（布尔）
    - `server_ts`：服务端当前时间戳（秒）
    - `owned_accounts`：当前用户可见账号数
    - `scanned_total`：本次扫描到的原始日志条数（过滤前）

### 4.1.2.31.1 `/wunder/channels/runtime_logs/probe`

- 方法：`POST`
- 说明：写入一条渠道运行测试日志，用于排查“面板无日志”与权限过滤问题。
- 入参（JSON）：
  - `channel`：渠道名（可选）
  - `account_id`：账号 ID（可选）
  - `agent_id`：智能体 ID（可选）
  - `message`：自定义日志内容（可选）
- 返回（JSON）：
  - `data.channel`：实际写入渠道
  - `data.account_id`：实际写入账号
  - `data.event`：固定 `runtime_probe`
  - `data.message`：日志内容
  - `data.ts`：写入时间戳（秒）
  - `data.status`：同 `/wunder/channels/runtime_logs` 的 `status` 字段

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
- `llm.models`：模型配置映射（model_type/provider/api_mode/base_url/api_key/model/temperature/timeout_s/retry/max_rounds/max_context/max_output/support_vision/support_hearing/stream/stream_include_usage/tool_call_mode/reasoning_effort/history_compaction_ratio/history_compaction_reset/stop/enable/mock_if_unconfigured）
  - 说明：`retry` 同时用于请求失败重试与流式断线重连。
  - 说明：`provider` 支持预置（`openai_compatible/openai/anthropic/openrouter/siliconflow/deepseek/moonshot/qwen/groq/mistral/together/ollama/lmstudio`）；`openai_compatible` 需显式填写 `base_url`，其余 provider 可省略 `base_url` 自动补齐。
  - 说明：`provider=anthropic` 使用 `/v1/messages` 协议，鉴权头为 `x-api-key`（同时兼容 `Authorization: Bearer`）。
  - 说明：`model_type=embedding` 表示嵌入模型，向量知识库会使用其 `/v1/embeddings` 能力。
  - 说明：`history_compaction_ratio` 默认 `0.9`，达到 `max_context * ratio` 后会优先触发预压缩。
  - 说明：`history_compaction_reset` 控制压缩后保留多少实时上下文：`zero` 仅保留压缩摘要继续推理；`current` 保留压缩摘要与当前用户问题；`keep` 额外保留最近用户消息窗口（最多约 20k token）。
  - 说明：`api_mode` 可选 `chat_completions|responses`（默认 chat_completions；当 provider=openai 且模型为 GPT-5/O 系列时未配置会自动走 responses），`responses` 会改用 `/v1/responses` 协议与流式事件。
  - 说明：`reasoning_effort` 可选 `none|minimal|low|medium|high|xhigh`；留空表示跟随模型默认思考等级。
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
- `security.external_auth_key`：外部系统嵌入登录密钥（为空时自动回退到 `security.api_key`）
- `security.external_embed_preset_agent_name`：外链嵌入预制智能体名称（为空表示未配置）
- `security.external_embed_jwt_secret`：外链 JWT 直登密钥（为空时自动回退到 `security.external_auth_key` / `security.api_key`）
- `security.external_embed_jwt_user_id_claim`：外链 JWT 中映射 wunder 用户 ID 的 claim 名称（默认 `sub`）
  - `security.allow_commands`：允许执行命令前缀列表
  - `security.allow_paths`：允许访问的额外目录列表；填 `*` 表示放开整个文件系统
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
  - `data.active`：当前启用的系统提示词模板包 ID（`default` 表示仓库内 `config/prompts/`）
  - `data.packs_root`：非 default 模板包的根目录（默认 `./config/data/prompt_templates`）
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

### 4.1.6.10 `/wunder/prompt_templates`

- 方法：`GET`
- 鉴权：用户侧 Bearer Token
- 返回（JSON）：
  - `data.active`：当前用户实际生效的模板包 ID
    - 默认会解析为 `default-zh` 或 `default-en`
    - 兼容别名 `default` 仅用于历史设置兼容，不再作为用户界面的主选项
  - `data.packs_root`：用户自定义模板包根目录
  - `data.default_sync_pack_id`：当前管理员启用的系统模板包 ID
  - `data.packs[]`：模板包列表
    - `id`：模板包 ID
    - `is_default`：是否为内置默认包
    - `readonly`：是否只读
    - `builtin`：是否为内置包
    - `locale`：内置包绑定语言，当前为 `zh` 或 `en`
    - `is_system_language_default`：是否为当前系统语言默认落点
    - `sync_pack_id`：内置包同步的系统模板包 ID
    - `path`：模板包路径
  - `data.segments[]`：可编辑分段列表
- 说明：
  - 用户侧默认提供 `default-zh` 与 `default-en` 两套只读内置模板包
  - 未显式选择时，后端会按当前系统语言把兼容别名 `default` 解析到对应内置包

### 4.1.6.11 `/wunder/prompt_templates/active`

- 方法：`POST`
- 鉴权：用户侧 Bearer Token
- 入参（JSON）：
  - `active`：要启用的模板包 ID，可为 `default-zh`、`default-en` 或自定义包 ID
- 返回（JSON）：
  - `ok`：是否成功
  - `data.active`：已保存的模板包 ID
- 说明：
  - 选中 `default-zh` 或 `default-en` 后，运行时会固定读取对应语言模板，不再随界面语言漂移

### 4.1.6.12 `/wunder/prompt_templates/file`

- 方法：`GET` / `PUT`
- 鉴权：用户侧 Bearer Token
- `GET` Query：
  - `pack_id`：模板包 ID，可选，默认读取当前启用包
  - `locale`：语言，可选；对内置包会被强制锁定为包绑定语言
  - `key`：分段 key
- `GET` 返回（JSON）：
  - `data.pack_id`：实际读取的模板包 ID
  - `data.locale`：实际读取语言
  - `data.key`：分段 key
  - `data.path`：实际读取路径
  - `data.exists`：当前包内该分段是否存在
  - `data.fallback_used`：是否回退到了系统模板内容
  - `data.readonly`：当前包是否只读
  - `data.source_pack_id`：实际命中的系统模板包 ID
  - `data.content`：分段内容
- `PUT` 入参（JSON）：
  - `pack_id`：模板包 ID
  - `locale`：语言
  - `key`：分段 key
  - `content`：分段内容
- `PUT` 返回（JSON）：
  - `ok`：是否成功
  - `data.pack_id`：模板包 ID
  - `data.locale`：写入语言
  - `data.key`：分段 key
  - `data.path`：写入路径
- 说明：
  - `default-zh` 与 `default-en` 为只读，禁止通过 `PUT` 修改
  - 自定义包缺失分段时，会先回退到当前系统 active 模板包，再回退到系统 `default`

### 4.1.6.13 `/wunder/prompt_templates/packs`

- 方法：`POST`
- 鉴权：用户侧 Bearer Token
- 入参（JSON）：
  - `pack_id`：要创建的自定义模板包 ID
  - `copy_from`：可选，复制来源模板包 ID
- 返回（JSON）：
  - `ok`：是否成功
  - `data.pack_id`：创建后的模板包 ID
  - `data.path`：模板包路径
  - `data.copied_from`：复制来源模板包 ID
- 说明：
  - `copy_from` 支持 `default-zh`、`default-en` 和任意现有自定义包
  - 对内置包复制时，会从当前管理员启用的系统模板内容复制出可编辑包

### 4.1.6.14 `/wunder/prompt_templates/packs/{pack_id}`

- 方法：`DELETE`
- 鉴权：用户侧 Bearer Token
- 返回（JSON）：
  - `ok`：是否成功
  - `data.pack_id`：删除的模板包 ID
- 说明：
  - 内置包 `default-zh` 与 `default-en` 不允许删除
  - 若删除的是当前启用的自定义包，后端会回退到系统语言对应的默认内置包

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
    + ttft_ms
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
    + ttft_ms
    + prefill_tokens/prefill_duration_s/prefill_speed_tps/prefill_speed_lower_bound
    + decode_tokens/decode_duration_s/decode_speed_tps）

### 4.1.9 `/wunder/admin/monitor/{session_id}`

- 方法：`GET`
- 返回（JSON）：
  - `session`：线程详情（start_time/session_id/user_id/question/status/token_usage/elapsed_s/stage/summary
    + ttft_ms
    + prefill_tokens/prefill_duration_s/prefill_speed_tps/prefill_speed_lower_bound
    + decode_tokens/decode_duration_s/decode_speed_tps）
  - `events`：事件详情列表
- 说明：
- `session` 详情新增 `log_profile`（`normal`/`debug`）与 `trace_id`，用于跨模块追踪。
- `session` 详情新增 `agent_name`（智能体名称），用于在线程详情中快速辨认线程归属。
- `events` 每条记录新增 `event_id`（线程内递增）。
- 每轮用户提问会额外写入 `user_input` 事件，`data.message/question` 保存原始用户消息，便于在线程详情中快速定位上下文。
- `normal` 日志画像会按 `observability.monitor_event_limit` 保留最近 N 条（<= 0 表示不截断），并按 `observability.monitor_payload_max_chars` 截断字符串字段（<= 0 表示不截断）。
- `normal` 日志画像默认跳过高频增量事件：`llm_output_delta`、`tool_output_delta`；`debug` 日志画像仅在管理员调试会话（`is_admin=true` 且 `debug_payload=true`）启用，并保留这些高频事件与完整字段。
- `llm_request` 事件仅保存 `payload_summary` 与 `message_count`，不保留完整请求体。
- `observability.monitor_drop_event_types` 主要作用于 `normal` 画像；`debug` 画像默认保留完整增量事件。
- 预填充速度基于会话第一轮 LLM 请求计算，避免多轮缓存导致速度偏高；当只能从“请求发出到首个输出事件”反推 TTFT 时，`prefill_speed_lower_bound=true`，表示该预填充速度是下界而非模型内部精确值。
- `session.context_tokens/context_tokens_peak` 汇总优先采用 `round_usage.total_tokens` 作为有效占用；`context_usage` 仍保留估算值用于过程观测。
- `round_usage.total_tokens` 表示单轮请求完成后的实际上下文占用，是当前线程上下文占用的权威口径；实际总消耗按每次请求的 `round_usage.total_tokens` 逐次累加。
- `round_usage` 事件额外提供 `context_occupancy_tokens` 与 `request_consumed_tokens` 两个显式语义别名；它们当前都与 `round_usage.total_tokens` 相同，新接入可直接按字段名区分“当前占用”和“单次请求消耗”。


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
- 说明：压缩重建默认保留“最近用户消息窗口（最多 20k token，按 token 窗口而非固定轮次）+ 压缩摘要 + 当前用户消息”；`compaction` 事件会额外包含 `recent_user_messages_retained`、`recent_user_tokens_retained` 与 `recent_user_window_token_limit` 字段。

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
- 渠道监控与治理：`/wunder/admin/channels/accounts`、`/wunder/admin/channels/accounts/batch`、`/wunder/admin/channels/accounts/{channel}/{account_id}`、`/wunder/admin/channels/accounts/{channel}/{account_id}/impact`、`/wunder/admin/channels/bindings`、`/wunder/admin/channels/user_bindings`、`/wunder/admin/channels/sessions`。
- 舰桥中心治理：`/wunder/admin/bridge/metadata`、`/wunder/admin/bridge/supported_channels`、`/wunder/admin/bridge/centers`、`/wunder/admin/bridge/centers/{center_id}`、`/wunder/admin/bridge/centers/{center_id}/accounts`、`/wunder/admin/bridge/centers/{center_id}/weixin_bind`、`/wunder/admin/bridge/accounts/{center_account_id}`、`/wunder/admin/bridge/routes`、`/wunder/admin/bridge/routes/{route_id}`、`/wunder/admin/bridge/delivery_logs`。
- 吞吐量/性能/benchmark/模拟：`/wunder/admin/throughput/*`、`/wunder/admin/performance/sample`、`/wunder/admin/benchmark/*`、`/wunder/admin/sim_lab/*`。
- 调试面板接口：`/wunder`、`/wunder/system_prompt`、`/wunder/tools`、`/wunder/attachments/convert`、`/wunder/workspace/*`、`/wunder/user_tools/*`、`/wunder/cron/*`。
- 文档/幻灯片：`/wunder/ppt`、`/wunder/ppt-en`。

### 4.1.24.4 `/wunder/admin/sim_lab/*`

- `GET /wunder/admin/sim_lab/projects`：获取模拟项目列表与默认参数。
- `POST /wunder/admin/sim_lab/runs`：执行模拟任务。
  - 入参（JSON）：`run_id`、`projects[]`、`options`。
  - `options.swarm_flow` 支持：`workers`、`max_wait_s`、`mother_wait_s`、`poll_ms`、`worker_task_rounds`、`keep_artifacts`、`strict_mock_only`。
  - `strict_mock_only` 默认 `true`：若检测到非本地 mock LLM 请求，当前模拟运行会直接失败并返回错误。
- `GET /wunder/admin/sim_lab/runs/{run_id}/status`：查询运行是否仍处于活动状态。
- `POST /wunder/admin/sim_lab/runs/{run_id}/cancel`：取消运行。
- 结果报告补充：`projects[].report.llm_request_audit` 与 `projects[].report.checks.mock_only_llm_requests`，用于判定是否全程仅命中 mock LLM 端点。

### 4.1.25 `/wunder/admin/tools`

- 方法：`GET/POST`
- `GET` 返回：
  - `enabled`：已启用内置工具名称列表
  - `tools`：内置工具列表（name/description/input_schema/enabled）
- `POST` 入参：
  - `enabled`：启用的内置工具名称列表

### 4.1.25.1 `/wunder/admin/channels/accounts`

- 方法：`GET`
- 入参（Query，可选）：
  - `channel`：渠道名过滤
  - `status`：账号状态过滤（如 `active`）
  - `keyword`：模糊搜索关键字（匹配渠道/账号/持有者/状态）
  - `owner_user_id`：按持有者用户 ID 过滤
  - `issue_only`：是否仅返回异常账号（`true/false`）
  - `last_active_after`：最近通信时间下限（秒级时间戳）
  - `last_active_before`：最近通信时间上限（秒级时间戳）
- 返回（JSON）：
  - `data.items[]`：渠道账号列表，字段包括：
    - `channel`、`account_id`、`status`、`config`、`created_at`、`updated_at`
    - `runtime`：运行态信息（当前含 `feishu_long_connection`、`xmpp_long_connection`）
    - `owner_user_id`、`owner_username`：主持有者（基于渠道用户绑定推导）
    - `owners[]`：持有者预览（`user_id`、`username`）
    - `owner_count`：持有者数量
    - `binding_count`：绑定数量
    - `session_count`：渠道会话数量
    - `message_count` / `inbound_message_count`：入站消息数量
    - `outbound_total_count` / `outbound_sent_count` / `outbound_failed_count` / `outbound_retry_count` / `outbound_pending_count`：出站分维统计
    - `outbound_retry_attempts`：累计出站重试次数
    - `outbound_success_rate`：出站成功率（`sent / (sent + failed)`）
    - `communication_count`：通信总量（入站 + 出站）
    - `last_communication_at`：最近通信时间（秒级时间戳）
    - `has_issue`：是否存在异常（账号停用、长连接异常或出站失败/重试）

### 4.1.25.1.1 `/wunder/admin/channels/accounts/batch`

- 方法：`POST`
- 入参（JSON）：
  - `action`：批量动作，支持 `enable` / `disable` / `delete`
  - `items[]`：目标账号列表
    - `channel`
    - `account_id`
- 返回（JSON）：
  - `data.action`：执行动作
  - `data.total`：请求中有效目标数（去重后）
  - `data.success` / `data.failed` / `data.skipped`：批量执行汇总
  - `data.deleted_accounts` / `data.deleted_bindings` / `data.deleted_user_bindings` / `data.deleted_sessions` / `data.deleted_messages` / `data.deleted_outbox`：当 `action=delete` 时的累计清理统计
  - `data.items[]`：逐账号结果（`channel/account_id/ok/result`，并按动作附带 `status` 或删除统计字段）
- 说明：用于管理员批量启用、停用或删除渠道账号；删除动作会复用单条删除链路并清理关联绑定、会话、消息与出站队列。

### 4.1.25.2 `/wunder/admin/channels/accounts/{channel}/{account_id}`

- 方法：`DELETE`
- 返回（JSON）：
  - `data.channel`、`data.account_id`
  - `data.deleted_accounts`：删除账号记录数
  - `data.deleted_bindings`：删除渠道绑定数
  - `data.deleted_user_bindings`：删除渠道用户绑定数
  - `data.deleted_sessions`：删除渠道会话数
  - `data.deleted_messages`：删除渠道消息数
  - `data.deleted_outbox`：删除出站队列记录数
- 说明：用于管理端快速移除失效渠道账号及其绑定关系。

### 4.1.25.2.1 `/wunder/admin/channels/accounts/{channel}/{account_id}/impact`

- 方法：`GET`
- 返回（JSON）：
  - `data.account_exists`：账号是否存在
  - `data.bindings`：将受影响的渠道绑定数
  - `data.user_bindings`：将受影响的渠道用户绑定数
  - `data.sessions`：将受影响的渠道会话数
  - `data.messages`：将受影响的渠道消息数
  - `data.outbox_total` / `data.outbox_pending` / `data.outbox_retry` / `data.outbox_failed`：将受影响的出站队列统计
- 说明：用于删除前影响预估提示。

### 4.1.25.3 `/wunder/admin/channels/bindings`

- 方法：`GET`
- 入参（Query，可选）：
  - `channel`：渠道名过滤
- 返回（JSON）：
  - `data.items[]`：渠道绑定列表（`binding_id/channel/account_id/peer_kind/peer_id/agent_id/tool_overrides/priority/enabled/created_at/updated_at`）

### 4.1.25.4 `/wunder/admin/channels/user_bindings`

- 方法：`GET`
- 入参（Query，可选）：
  - `channel`、`account_id`、`peer_kind`、`peer_id`、`user_id`
  - `offset`（默认 0）、`limit`（默认 50）
- 返回（JSON）：
  - `data.items[]`：用户绑定列表（`channel/account_id/peer_kind/peer_id/user_id/created_at/updated_at`）
  - `data.total`：总数

### 4.1.25.5 `/wunder/admin/channels/sessions`

- 方法：`GET`
- 入参（Query，可选）：
  - `channel`、`account_id`、`peer_id`、`session_id`
  - `offset`（默认 0）、`limit`（默认 50）
- 返回（JSON）：
  - `data.items[]`：会话列表（`channel/account_id/peer_kind/peer_id/thread_id/session_id/agent_id/user_id/tts_enabled/tts_voice/metadata/last_message_at/created_at/updated_at`）
  - `data.total`：总数

### 4.1.25.6 `/wunder/admin/bridge/metadata`

- 方法：`GET`
- 返回（JSON）：
  - `data.default_password`：舰桥节点自动开户默认密码（当前固定 `123456`）
  - `data.supported_channels[]`：支持挂入舰桥节点的渠道清单（含 `channel/display_name/webhook_mode/adapter_registered/provider_caps`）
  - `data.preset_agents[]`：可选默认预设智能体（`name/description`）
  - `data.channel_accounts[]`：当前系统已激活的共享渠道账号（`channel/account_id/status`）
  - `data.org_units[]`：可选目标单位（`unit_id/name/path_name/level`）

### 4.1.25.7 `/wunder/admin/bridge/centers`

- 方法：`GET/POST`
- `GET` 入参（Query，可选）：
  - `status`：中心状态过滤
  - `keyword`：名称/编码模糊搜索
  - `offset`、`limit`
- `GET` 返回：
  - `data.items[]`：舰桥节点列表，字段包括 `center_id/name/code/status/default_preset_agent_name/default_identity_strategy/username_policy/account_count/shared_channel_count/route_count/active_route_count/owner_user_id/owner_username`
- `POST` 入参（JSON）：
  - `center_id`：可选，传入时为更新
  - `name`、`code`
  - `status`
  - `default_preset_agent_name`
  - `target_unit_id`
  - `default_identity_strategy`
  - `username_policy`
  - `description`
  - `shared_channels[]`：可选的批量写入能力；当前单个舰桥节点只允许一个渠道，管理端页面默认不走一次性保存，而是通过“渠道设置”弹窗维护单条绑定
- 说明：管理员用它创建或更新一个“全渠道入口 -> 默认预设智能体”的舰桥节点。页面当前采用“监控主页面 + 中心配置弹窗 + 渠道设置弹窗”模式。

### 4.1.25.8 `/wunder/admin/bridge/centers/{center_id}`

- 方法：`GET/DELETE`
- `GET` 返回：
  - `data.center`：中心详情
  - `data.shared_channels[]`：该中心下的接入渠道配置
  - `data.accounts[]`：该中心下的共享渠道账号配置
- `DELETE` 返回：
  - `data.deleted`：删除中心记录数；关联 `bridge_center_accounts / bridge_user_routes / bridge_delivery_logs / bridge_route_audit_logs` 会同步清理

### 4.1.25.9 `/wunder/admin/bridge/centers/{center_id}/accounts`

- 方法：`GET/POST`
- `GET` 返回：
  - `data.items[]`：共享渠道账号列表（`center_account_id/channel/account_id/enabled/identity_strategy/thread_strategy/default_preset_agent_name_override/route_count/active_route_count/provider_caps`）
- `POST` 入参（JSON）：
  - `channel`、`account_id`
  - `enabled`
  - `identity_strategy`
  - `thread_strategy`
  - `reply_strategy`
  - `default_preset_agent_name_override`
- 说明：底层仍保留独立渠道绑定接口，便于脚本化接线；当前单个舰桥节点只允许绑定一个渠道账号。管理端页面会先通过 `/wunder/admin/channels/accounts?status=active` 拉取现有可用账号，再用此接口写入桥接绑定。

### 4.1.25.10 `/wunder/admin/bridge/accounts/{center_account_id}`

- 方法：`PATCH/DELETE`
- `PATCH`：更新某个共享渠道账号配置，入参与 `POST /wunder/admin/bridge/centers/{center_id}/accounts` 相同。
- `DELETE`：删除该共享账号，并清理其名下 bridge routes、delivery logs、audit logs。

### 4.1.25.10A `/wunder/admin/bridge/centers/{center_id}/weixin_bind`

- 方法：`POST`
- 入参（JSON）：
  - `account_id`：可选；为空时服务端按节点自动生成稳定的 Weixin 账号 ID
  - `api_base`
  - `bot_type`
  - `bot_token`
  - `ilink_bot_id`
  - `ilink_user_id`
- 返回（JSON）：
  - `data.center`：所属舰桥节点
  - `data.account`：最终写入的 `bridge_center_account`
  - `data.channel_account`：最终写入的 `channel_accounts.weixin/*` 配置快照
- 说明：管理员侧 `Weixin iLink (New)` 扫码流程会先调用已有 `/wunder/channels/weixin/qr/start`、`/wunder/channels/weixin/qr/wait` 获取二维码和扫码确认结果，再调用这里把凭据落成真实渠道账号并绑定到当前舰桥节点；如果节点已有旧绑定，会先清理旧 bridge routes / delivery logs / audit logs。

### 4.1.25.11 `/wunder/admin/bridge/routes`

- 方法：`GET`
- 入参（Query，可选）：
  - `center_id`、`center_account_id`
  - `channel`、`account_id`
  - `status`
  - `keyword`
  - `wunder_user_id`、`agent_id`
  - `offset`、`limit`
- 返回（JSON）：
  - `data.items[]`：自动分配路由列表，字段包括 `route_id/external_identity_key/external_display_name/wunder_user_id/wunder_username/agent_id/agent_name/status/last_session_id/last_error/last_inbound_at/last_outbound_at`

### 4.1.25.12 `/wunder/admin/bridge/routes/{route_id}`

- 方法：`GET/PATCH`
- `GET` 返回：
  - `data.route`：单条 bridge route 详情
  - `data.delivery_logs[]`：最近投递日志
  - `data.audit_logs[]`：最近治理审计日志
- `PATCH` 入参（JSON）：
  - `status`：可切换到 `active/paused/blocked/error`
  - `clear_last_error`：是否清空 `last_error`
- 说明：用于暂停、恢复或封禁某条外部用户自动分配路由。

### 4.1.25.13 `/wunder/admin/bridge/delivery_logs`

- 方法：`GET`
- 入参（Query，可选）：
  - `center_id`、`center_account_id`、`route_id`
  - `direction`：`inbound/outbound`
  - `status`
  - `limit`
- 返回（JSON）：
  - `data.items[]`：投递日志列表（`delivery_id/direction/stage/status/provider_message_id/session_id/summary/payload/created_at`）

### 4.1.26 `/wunder/admin/knowledge`

- 方法：`GET/POST`
- `GET` 返回：
  - `knowledge`：知识库配置（bases 数组，元素包含 name/description/root/enabled/base_type/embedding_model/chunk_size/chunk_overlap/top_k/score_threshold）
- `POST` 入参：
  - `knowledge`：完整知识库配置，用于保存与下发
- 说明：当 root 为空时，字面知识库会自动创建 `./config/knowledge/<知识库名称>` 目录；向量知识库 root 自动指向 `config/data/vector_knowledge/shared/<base>` 作为逻辑标识，文档与切片元数据存储在数据库中，并要求 `embedding_model`

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
    - `text`：模型正式输出
    - `reasoning`：模型返回的思考过程
    - `hits`：命中文档列表
      - `doc_id`：文档编码
      - `document`：文档名称
      - `content`：文档内容
      - `score`：相关度分数（可选）
      - `section_path`：章节路径
      - `reason`：命中原因（可选）
- 说明：字面知识库会调用大模型生成原始输出，并附带命中文档内容；向量知识库保持召回结果。

### 4.1.30.7.1 `/wunder/admin/knowledge/test/stream`

- 方法：`POST`
- 入参（JSON）：
  - `base`：知识库名称
  - `query`：测试问题
  - `top_k`：召回数量（可选，默认使用知识库配置）
- 返回：`text/event-stream`
  - `event: request`
    - 字面知识库：完整 LLM 请求体，包含 `payload`、`base_url`、候选片段数量等调试信息
    - 向量知识库：当前检索请求摘要，包含 `embedding_model`、`top_k` 等参数
  - `event: reasoning`
    - `delta`：模型思考增量，仅字面知识库返回
  - `event: output`
    - `delta`：模型正式输出增量，仅字面知识库返回
  - `event: complete`
    - 向量知识库：`base`、`query`、`embedding_model`、`top_k`、`hits`
    - 字面知识库：`base`、`query`、`text`、`reasoning`、`hits`
  - `event: error`
    - `message`：错误信息
- 说明：管理员侧“知识库测试”弹窗对字面知识库使用该接口流式展示完整请求体、思考过程与正式输出，便于定位检索慢或回答异常问题。

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
- `memory_manager` 建议主动触发时机：当模型置信度不足、信息疑似过期、用户指出“答错/记错”、或用户反馈导致偏好/约束变化时，先执行 `recall` 校验，再决定是否 `add/update`。
- recall 目前仅保留轻量关键词召回，不再使用 embedding/语义 rerank；工具返回会收敛为更适合模型消费的精简结构（如 `matched_terms`、`why`），以降低上下文开销。
- 会话发生 context compaction 后，调度器会基于 `用户 + 智能体 + 当前问题` 再次执行 fresh recall，并把记忆块拼接到压缩摘要消息继续执行（不改写线程冻结的 system prompt）；记忆块会额外带上“当前可用总条数 / 本次注入条数 / 注入上限”摘要，并在必要时提示模型可继续通过 `memory_manager recall/list` 检索剩余记忆；`compaction` 事件会附带 `fresh_memory_injected`、`fresh_memory_count` 与 `fresh_memory_total_count` 字段。
- 记忆碎片当前可见状态为 `active / superseded / invalidated`；其中 `superseded` 表示该碎片已被同 `fact_key` 的新版本替代，默认不会被 recall 返回，但仍会在列表接口与用户可视化卡片墙中展示。
- recall 命中、碎片创建/编辑、列表读取时会惰性刷新 `tier(core/working/peripheral)` 与状态链路；因此接口返回的 `tier`、`status`、`supersedes_memory_id`、`superseded_by_memory_id` 字段可直接用于前端展示版本关系与生命周期信息。
- `memory_manager` 的 `list/add/update/delete/clear/recall` 已与结构化 `memory_fragments` 共用同一条主存储链路；模型经工具写入的新记忆会直接出现在用户侧“记忆碎片”卡片页，无需再等待旧摘要表懒迁移。
- `confirmed_by_user` 字段当前仅作为兼容旧数据保留，不再作为用户侧记忆碎片页面的交互入口，也不再参与 recall 排序和提示词快照构建。
- 自动记忆提炼改为按 `用户 + 智能体` 单独开关，默认关闭。只有在用户侧“记忆碎片 -> 最近提炼任务”弹窗中显式开启后，系统才会在每个用户轮次结束并发出 `final` 回复后异步尝试写入 `auto-turn` 记忆；提炼阶段走独立的大模型提示词 `config/prompts/{zh|en}/memory_auto_extract.txt`，而最终写入前仍由服务端执行去重、`fact_key` 版本替代与手工/置顶碎片保护。
- 聊天页提示词预览接口 `/wunder/chat/system-prompt` 与 `/wunder/chat/sessions/{session_id}/system-prompt` 现会额外返回 `memory_preview`、`memory_preview_mode(frozen/pending/none)`、`memory_preview_count`、`memory_preview_total_count`，用于向用户明确展示“当前线程已冻结”或“新线程将注入”的记忆快照；其中 `memory_preview_count` 表示当前提示词里实际注入的记忆条数，`memory_preview_total_count` 表示该记忆块生成时可用的长期记忆总数；新建线程在首条用户消息前应为 `pending`，首条用户消息发送后才转为 `frozen`。

#### `GET /wunder/agents/{agent_id}/memory-settings`

- 返回当前用户在指定智能体上的记忆设置。
- 响应示例：

```json
{
  "data": {
    "settings": {
      "auto_extract_enabled": false,
      "updated_at": 0
    }
  }
}
```

#### `POST /wunder/agents/{agent_id}/memory-settings`

- 更新当前用户在指定智能体上的记忆设置。
- 请求体：

```json
{
  "auto_extract_enabled": true
}
```

- 响应示例：

```json
{
  "data": {
    "settings": {
      "auto_extract_enabled": true,
      "updated_at": 1773620812.637
    }
  }
}
```

- `GET /wunder/agents/{agent_id}/memories` 的响应体中也会附带同一份 `data.settings`，便于前端在记忆卡片页一次请求同时渲染列表、命中记录、提炼任务与设置开关。

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
- 说明：报告会持久化到 `config/data/throughput`，便于导出与回溯。

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
  - `ttft_ms`：TTFT/首 token 到达延迟（毫秒）
  - `first_token_latency_ms`：首包延迟（毫秒）
  - 兼容说明：`first_token_latency_ms` 当前与 `ttft_ms` 等价，保留给旧版调用方。
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
- `ttft_ms`：该档位 TTFT（毫秒）
- `p50_latency_ms/p90_latency_ms/p99_latency_ms`：延迟分位（毫秒）
- `total_prefill_speed_tps`：总预填充速度（token/s）
- `single_prefill_speed_tps`：单预填充速度（token/s）
- `total_decode_speed_tps`：总解码速度（token/s，按该档位 `decode_tokens_total / elapsed_s` 计算）
- `single_decode_speed_tps`：并发平均解码速度（token/s，按每请求 `decode_tokens / request_elapsed_s` 算术平均，包含排队等待与首包等待）
- `total_decode_speed_stream_chunk_tps`：流分片近似总解码速度（token/s，按该档位 `llm_output_delta(delta/reasoning_delta 文本) 近似 token 总量 / elapsed_s` 计算）
- `single_decode_speed_stream_chunk_tps`：流分片近似并发平均解码速度（token/s，按每请求 `llm_output_delta(delta/reasoning_delta 文本) 近似 token 数 / request_elapsed_s` 算术平均）
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

## 2026-03-22 增补：weixin（微信 iLink）渠道接入（P0）

- 新增渠道 provider：`weixin`（独立于 `wechat` / `wechat_mp`）。
- 运行形态：长轮询 worker（`ilink/bot/getupdates`），不是平台 webhook 回调模式。
- 出站发送：`ilink/bot/sendmessage`，回复必须携带 `context_token`。
- 配置入口：`/wunder/channels/accounts`，账号配置键为 `weixin.*`（`api_base`、`bot_token`、`ilink_bot_id` 等）。
- 管理端运行态：`/wunder/admin/channels/accounts` 增加 `weixin_long_connection` 状态字段。

## 2026-03-22 增补：weixin（微信 iLink）渠道接入（P1）

- 媒体出站链路（已接入）：`ilink/bot/getuploadurl` + CDN `/upload`（AES-128-ECB + PKCS7），再通过 `ilink/bot/sendmessage` 发送 `image/file/video/voice` item。
- 媒体入站链路（已接入）：`getupdates.msgs[*].item_list` 中的媒体项会下载 CDN `/download`，按 `media.aes_key`（兼容 base64(raw16) 与 base64(hex32)）解密后落地到工作区，并回写附件 URL。
- `weixin` 新增可选配置键：
  - `weixin.cdn_base`：CDN 基地址（默认 `https://novac2c.cdn.weixin.qq.com/c2c`）
  - `weixin.bot_type`：二维码登录 bot_type（默认 `3`）
- 新增二维码登录接口：
  - `POST /wunder/channels/weixin/qr/start`
    - 入参：`account_id?`、`api_base?`、`bot_type?`、`force?`
    - 返回：`session_key`、`qrcode`、`qrcode_url`、`qrcode_open_url`、`api_base`、`bot_type`
  - `POST /wunder/channels/weixin/qr/wait`
    - 入参：`session_key`、`api_base?`、`timeout_ms?`
    - 返回：`connected`、`status`，若确认登录则附带 `bot_token`、`ilink_bot_id`、`ilink_user_id`、`api_base`
  - `GET /wunder/channels/weixin/qr/render`
    - 入参（Query）：`text`（必填），`api_base?`
    - 返回：`image/png`；用于前端兜底渲染二维码，避免外部 H5 链接直接作为 `<img>` 导致破图

## 2026-03-24 增补：聊天消息反馈（点赞/踩）

### `POST /wunder/chat/sessions/{session_id}/messages/{history_id}/feedback`

- 方法：`POST`
- 鉴权：用户侧 Bearer Token
- 入参（JSON）：
  - `vote`：`up` / `down`（支持 `like/dislike/thumb_up/thumb_down` 等兼容写法）
- 约束：
  - 仅允许对 `assistant` 消息提交反馈。
  - 同一条消息只允许提交一次；提交后锁定，不可修改。
- 返回（JSON）：
  - `data.session_id`：会话 ID
  - `data.history_id`：消息 history_id
  - `data.feedback`：
    - `vote`：`up` / `down`
    - `created_at`：反馈时间（RFC3339）
    - `locked`：固定为 `true`
- 错误码：
  - `400`：参数非法或目标不是 assistant 消息
  - `404`：会话或消息不存在
  - `409`：该消息已存在反馈（已锁定）

### 会话消息返回体补充（用户侧聊天接口）

- `GET /wunder/chat/sessions/{session_id}`
- `GET /wunder/chat/sessions/{session_id}/history`
- `GET /wunder/chat/sessions/{session_id}` 新增 `data.agent_name`（智能体名称，默认智能体同样返回名称）。
- `GET /wunder/chat/sessions/{session_id}` 新增 `data.context_occupancy_tokens`，作为 `data.context_tokens` 的显式语义别名；新接入优先使用该字段表达当前线程上下文占用。
- 当会话仅处于队列等待阶段、最新用户消息尚未落入历史时，`GET /wunder/chat/sessions/{session_id}` 会基于活跃 `agent_tasks` 追加一组临时消息视图：
  - 最新用户消息会按请求体中的 `question/attachments` 投影到 `data.messages[]`
  - 对应助手占位会带 `stream_incomplete=true`
  - 对应助手占位会附带队列 workflow 事件（如 `queue_enter`，必要时包含 `queue_start`），便于刷新后立即恢复“排队中/开始处理”的可见状态
  - 该投影仅用于刷新/重连后的实时态恢复，不写回历史；一旦真实历史落库，会以真实消息为准
- 当消息为 `assistant` 且已反馈时，`messages[].feedback` 结构如下：
  - `vote`：`up` / `down`
  - `created_at`：反馈时间（RFC3339）
  - `locked`：`true`
- 当消息为 `assistant` 且由该条回复触发过子智能体派发时，`messages[].subagents[]` 会返回当前已知的子智能体运行项；典型字段包括 `session_id/run_id/dispatch_id/title/label/status/summary/terminal/failed/can_terminate/updated_at/parent_user_round/parent_model_round/agent_state/detail`。
- 当消息为系统内部补发的隐藏观察消息时，`messages[].hiddenInternal=true`；该消息仅用于保持父子轮次与自动唤醒链路一致，前端默认应跳过渲染正文。

### 监控接口补充（管理员侧）

- `GET /wunder/admin/monitor` 的 `sessions[]` 新增：
  - `feedback_up_count`：点赞数
  - `feedback_down_count`：点踩数
  - `feedback_total_count`：反馈总数
  - `feedback_status`：`up` / `down` / `mixed` / `none`
- `GET /wunder/admin/monitor` 与 `GET /wunder/admin/monitor/{session_id}` 现额外提供：
  - `context_occupancy_tokens`：`context_tokens` 的显式语义别名
  - `context_occupancy_tokens_peak`：`context_tokens_peak` 的显式语义别名
- `GET /wunder/admin/monitor/{session_id}` 新增：
  - `feedback[]`：线程反馈列表
    - `history_id`：消息 history_id
    - `vote`：`up` / `down`
    - `user_id`：提交反馈的用户 ID
    - `created_at`：反馈时间（RFC3339）
    - `created_time`：UNIX 时间戳（秒）

## 2026-03-26 增补：beeroom 实时链路重构与观测

### `GET /wunder/beeroom/realtime/metrics`

- 方法：`GET`
- 鉴权：与 beeroom WS/SSE 保持一致（用户鉴权，支持 query token）
- 返回（JSON）：
  - `metrics.publish_total`：实时事件发布总数
  - `metrics.replay_batch_total`：回放批次数
  - `metrics.replay_event_total`：回放事件总量
  - `metrics.replay_failure_total`：回放失败次数
  - `metrics.lag_recovery_total`：lag 恢复次数
  - `metrics.push_sample_total`：推送延迟采样数
  - `metrics.push_latency_avg_ms`：推送延迟均值（ms）
  - `metrics.push_latency_max_ms`：推送延迟最大值（ms）
  - `timestamp`：服务端时间（RFC3339）

### beeroom WS/SSE watch 语义更新

- `watch` 进入后不再只依赖内存广播，先按 `after_event_id` 回放持久化事件，再进入 live push。
- 当连接出现 `Lagged` 或续传缺口时，服务端优先做 cursor replay 补齐，而不是直接要求前端全量刷新。
- `sync_required` 仍保留为兜底事件，但主恢复路径已切换为“回放优先”。
- 前端 beeroom 轮询降级为健康检查（默认 30s），不再作为主实时来源。

### 说明

- 本次改造针对 beeroom/chat/channel 的消息实时链路；“模型配置变更（用户改模型、管理员改默认模型）”的全局推送链路尚未迁移到统一实时总线，仍按现有页面刷新/重新拉取机制生效。

## 2026-04-04 增补：聊天附件预处理

### `POST /wunder/chat/attachments/convert`

- 方法：`POST`
- 鉴权：与聊天域保持一致（用户侧 Bearer Token）
- Body：`multipart/form-data`
  - `file`：必填，可上传一个或多个文档附件；支持范围与 `/wunder/doc2md/convert` 一致
- 返回：`JSON`
  - 单文件时：`data.name/content/converter/warnings`
  - 多文件时：`data.items[]`
- 说明：
  - 这是聊天域的文档预处理入口，供前端先把文档转成文本型附件，再提交到 `POST /wunder/chat/sessions/{session_id}/messages`
  - 图片一般直接走 `attachments[]`
  - 音频 / 视频走 `/wunder/chat/attachments/media/process`

### `POST /wunder/chat/attachments/media/process`

- 方法：`POST`
- 鉴权：用户侧 Bearer Token
- Body：`multipart/form-data`
  - 首次处理：
    - `file`：必填，音频或视频文件
    - `frame_rate`：可选，视频抽帧频率（FPS），默认 `1`
  - 重新抽帧：
    - `source_public_path`：必填，之前返回的源媒体工作区公共路径
    - `frame_rate`：必填或可选，视频抽帧频率（FPS）
- 支持类型：
  - 音频：`mp3/wav/ogg/opus/aac/flac/m4a/webm(audio/*)`
  - 视频：`mp4/mov/mkv/avi/webm(video/*)/mpeg/mpg/m4v`
- 返回：`JSON`
  - `data.kind`：`audio` / `video`
  - `data.name`：源文件名
  - `data.source_public_path`：源媒体落盘后的 `/workspaces/...` 公共路径，可用于后续重新抽帧
  - `data.duration_ms`：媒体时长（若可探测）
  - `data.requested_frame_rate`：请求的 FPS（仅视频）
  - `data.applied_frame_rate`：实际采用的 FPS（仅视频，超长视频会被自动下调）
  - `data.frame_count`：返回的图片帧数量（仅视频）
  - `data.has_audio`：视频是否成功抽取到音轨
  - `data.warnings[]`：降级说明，例如 ASR 未配置、视频无音轨、抽帧被限流
  - `data.attachments[]`：可直接作为聊天附件提交的派生结果
    - 图片帧：`name/content_type=image/jpeg/public_path`
    - 音频结果：`name/content(转写文本或占位文本)/content_type/public_path`
- 说明：
  - 视频不会直接作为模型输入，而是先拆成图片序列和音轨。
  - 默认每秒抽 `1` 帧，并带总帧数上限保护；超长视频会自动降低实际 FPS。
  - 音频/视频转写复用 `channels.media.asr` 配置；若未启用 ASR，接口仍会成功返回，但会在 `warnings` 中说明，并给音频附件写入占位文本。
  - 运行该接口需要服务端可用的 `ffmpeg/ffprobe`；也可通过环境变量 `WUNDER_FFMPEG_BIN`、`WUNDER_FFPROBE_BIN` 指定路径。

### 聊天消息提交补充

- `POST /wunder/chat/sessions/{session_id}/messages`
- 现支持“仅附件、无正文”的提交方式：
  - 只要 `attachments[]` 中存在非空 `content` 或 `public_path`，即可不传文本正文。
  - 这同样适用于图片、文档、音频转写结果以及视频拆帧结果。

- 工具结果现在采用“双通道”：
  - 前端事件通道（`tool_result` SSE）保留结构化结果用于渲染与工作流关联：`tool/ok/data/tool_call_id`，并保留 `meta` 与失败关键信息（`error/error_code/retryable`）；仍会裁剪 `trace_id/user_round/model_round` 等轮次追踪噪声。
  - 模型 observation 通道走极简压缩：在不破坏可继续执行语义的前提下移除冗余字段并压缩大体积结构。
- `tool_result` 事件额外提供 `model_observation`（JSONL 文本，单行 JSON），其内容与本轮实际送入模型的 observation 对齐，可直接用于前端排障展示。
- 为减少模型上下文占用，observation 压缩阶段会将 `data` 下密集数组压缩为 JSONL 字段（如 `hits_jsonl/matches_jsonl/files_jsonl/rows_jsonl`）并附带 `*_count` 计数。
- When truncation happens, payload/meta may include `truncation_reasons` (for example `array_items`/`string_chars`/`char_budget`), and multiple reasons can co-exist in the same result to indicate compound truncation.
