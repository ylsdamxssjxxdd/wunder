# wunder API 文档

## 4. API 设计

### 4.0 实现说明

- 接口实现按 core/admin/workspace/user_tools 路由模块拆分，统一由 `app/services` 提供配置与工具装配服务。
- 工具清单与提示词注入共享统一的工具规格构建逻辑，确保输出一致性。
- 启动优化：MCP 服务、监控与调度器采用惰性初始化，首次访问相关接口可能有冷启动延迟。
- 轻量入口：推荐使用 `uvicorn app.asgi:app` 启动，可通过 `WUNDER_LAZY_WARMUP_S` 控制后台预热延迟（秒，负数/关闭值表示禁用预热）。
- 配置分层：基础配置为 `config/wunder.yaml`（`WUNDER_CONFIG_PATH` 可覆盖），管理端修改会写入 `data/config/wunder.override.yaml`（`WUNDER_CONFIG_OVERRIDE_PATH` 可覆盖）。
- 鉴权：所有 `/wunder` 与 `/wunder/mcp` 请求需在请求头携带 `X-API-Key` 或 `Authorization: Bearer <key>`，配置项为 `config/wunder.yaml` 的 `security.api_key`。

### 4.1 `/wunder` 请求

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：字符串，用户唯一标识
  - `question`：字符串，用户问题
  - `tool_names`：字符串列表，可选，指定启用的内置工具/MCP/技能名称
  - `stream`：布尔，可选，是否流式输出（默认 true）
  - `session_id`：字符串，可选，指定会话标识
  - `model_name`：字符串，可选，模型配置名称（不传则使用默认模型）
- `config_overrides`：对象，可选，用于临时覆盖配置
- `attachments`：数组，可选，附件列表（文件为 Markdown 文本，图片为 data URL）
- 约束：同一 `user_id` 若已有运行中的会话，接口返回 429 并提示稍后再试。
- 约束：全局并发上限由 `server.max_active_sessions` 控制，超过上限的请求会排队等待。

### 4.1.1 `/wunder/system_prompt`

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：字符串，用户唯一标识
  - `session_id`：字符串，可选，会话标识
  - `tool_names`：字符串列表，可选，指定启用的内置工具/MCP/技能名称
  - `config_overrides`：对象，可选，用于临时覆盖配置
- 返回（JSON）：
  - `prompt`：字符串，当前系统提示词
  - `build_time_ms`：数字，系统提示词构建耗时（毫秒）

### 4.1.2 `/wunder/tools`

- 方法：`GET`
- 入参（Query）：
  - `user_id`：字符串，可选，用户唯一标识（传入后返回自建/共享工具与附加提示词）
- 返回（JSON）：
  - `builtin_tools`：内置工具列表（name/description/input_schema）
  - `mcp_tools`：MCP 工具列表（name/description/input_schema）
  - `skills`：技能列表（name/description/input_schema）
  - `knowledge_tools`：字面知识库工具列表（name/description/input_schema）
  - `user_tools`：自建工具列表（name/description/input_schema）
  - `shared_tools`：共享工具列表（name/description/input_schema/owner_id）
  - `extra_prompt`：附加提示词文本（与用户自建工具配置关联）
- 说明：
  - 自建/共享工具名称统一为 `user_id@工具名`（MCP 为 `user_id@server@tool`）。

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

- 方法：`GET/POST`
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

### 4.1.2.4 `/wunder/user_tools/skills/content`

- 方法：`GET`
- 入参（Query）：
  - `user_id`：用户唯一标识
  - `name`：技能名称
- 返回（JSON）：
  - `name`：技能名称
  - `path`：SKILL.md 文件路径
  - `content`：SKILL.md 完整内容

### 4.1.2.5 `/wunder/user_tools/skills/upload`

- 方法：`POST`
- 入参：`multipart/form-data`
  - `file`：技能 zip 压缩包
- 返回（JSON）：
  - `ok`：是否成功
  - `extracted`：解压文件数量
  - `message`：提示信息

### 4.1.2.6 `/wunder/user_tools/knowledge`

- 方法：`GET/POST`
- `GET` 入参（Query）：
  - `user_id`：用户唯一标识
- `GET` 返回（JSON）：
  - `knowledge.bases`：知识库列表（name/description/root/enabled/shared）
- `POST` 入参（JSON）：
  - `user_id`：用户唯一标识
  - `knowledge.bases`：知识库列表（name/description/enabled/shared，root 由系统固定生成）
- `POST` 返回：同 `GET`

### 4.1.2.7 `/wunder/user_tools/knowledge/files`

- 方法：`GET`
- 入参（Query）：
  - `user_id`：用户唯一标识
  - `base`：知识库名称
- 返回（JSON）：
  - `base`：知识库名称
  - `files`：Markdown 文件相对路径列表

### 4.1.2.8 `/wunder/user_tools/knowledge/file`

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

### 4.1.2.9 `/wunder/user_tools/knowledge/upload`

- 方法：`POST`
- 入参（multipart/form-data）：
  - `user_id`：用户唯一标识
  - `base`：知识库名称
  - `file`：待上传文件
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
  - `path`：转换后的 Markdown 相对路径
  - `converter`：使用的转换器（doc2md/python-xxx）
  - `warnings`：转换警告列表
- 说明：支持 doc2md README 中列出的扩展名；若 doc2md 不可用或执行失败，将尝试使用 Python 库进行兜底转换。

### 4.1.2.10 `/wunder/user_tools/extra_prompt`

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：用户唯一标识
  - `extra_prompt`：附加提示词文本
- 返回（JSON）：
  - `user_id`：用户唯一标识
  - `extra_prompt`：附加提示词文本

### 4.1.2.11 `/wunder/attachments/convert`

- 方法：`POST`
- 入参：`multipart/form-data`
  - `file`：待解析文件
- 返回（JSON）：
  - `ok`：是否成功
  - `name`：文件名
  - `content`：解析后的 Markdown 文本
  - `converter`：转换器（doc2md/python-xxx）
  - `warnings`：转换警告列表
- 说明：仅用于调试面板附件解析，支持 doc2md README 中列出的扩展名。

### 4.1.2.12 `/wunder/mcp`

- 类型：MCP 服务（streamable-http）
- 说明：系统自托管 MCP 入口，默认在管理员 MCP 服务管理中内置但未启用。
- 鉴权：请求头需携带 `X-API-Key` 或 `Authorization: Bearer <key>`。
- 工具：`wunder@run`
  - 入参：`task` 字符串，任务描述
  - 行为：使用固定 `user_id = wunder` 执行任务，按管理员启用的工具清单运行，并剔除 `wunder@run` 避免递归调用
  - 返回：`answer`/`session_id`/`usage`
- 参考配置：`endpoint` 默认可设为 `${WUNDER_MCP_ENDPOINT:-http://127.0.0.1:8000/wunder/mcp}`
- 超时配置：MCP 调用全局超时由 `config.mcp.timeout_s` 控制（秒）

### 4.1.3 `/wunder/admin/mcp`

- 方法：`GET/POST`
- `GET` 返回：
  - `servers`：MCP 服务列表（name/endpoint/allow_tools/enabled）
- `POST` 入参：
  - `servers`：完整 MCP 服务列表，用于保存配置

### 4.1.4 `/wunder/admin/mcp/tools`

- 方法：`POST`
- 入参（JSON）：
  - `name`：服务名称
  - `endpoint`：服务地址
- 返回（JSON）：
  - `tools`：服务端工具清单

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

### 4.1.6 `/wunder/admin/llm`

- 方法：`GET/POST`
- `GET` 返回：
  - `llm.default`：默认模型配置名称
- `llm.models`：模型配置映射（provider/base_url/api_key/model/temperature/timeout_s/retry/max_rounds/max_context/max_output/support_vision/stream/stream_include_usage/history_compaction_ratio/history_compaction_reset/stop/enable/mock_if_unconfigured）
  - 说明：`retry` 同时用于请求失败重试与流式断线重连。
- `POST` 入参：
  - `llm.default`：默认模型配置名称
  - `llm.models`：模型配置映射，用于保存与下发

### 4.1.6.1 `/wunder/admin/llm/context_window`

- 方法：`POST`
- 入参（JSON）：
  - `provider`：模型提供方类型（默认 openai_compatible）
  - `base_url`：模型服务地址
  - `api_key`：访问密钥（可选）
  - `model`：模型名称
  - `timeout_s`：探测超时秒数（可选）
- 返回（JSON）：
  - `max_context`：最大上下文长度（可能为 null）
  - `message`：探测结果说明

### 4.1.7 `/wunder/admin/skills/upload`

- 方法：`POST`
- 入参：`multipart/form-data`
  - `file`：技能 zip 压缩包
- 返回（JSON）：
  - `ok`：是否成功
  - `extracted`：解压文件数量

### 4.1.8 `/wunder/admin/monitor`

- 方法：`GET`
- 入参（Query）：
  - `active_only`：是否仅返回活动线程（默认 true）
  - `tool_hours`：统计窗口（小时，可选，用于工具热力图与近完成统计）
  - `start_time`：筛选开始时间戳（秒，可选，与 `end_time` 搭配时按区间统计）
  - `end_time`：筛选结束时间戳（秒，可选，与 `start_time` 搭配时按区间统计）
- 说明：当提供 `start_time`/`end_time` 时，将按区间统计并忽略 `tool_hours`。
- 返回（JSON）：
  - `system`：系统资源占用（cpu_percent/memory_total/memory_used/memory_available/process_rss/process_cpu_percent/load_avg_1/load_avg_5/load_avg_15/disk_total/disk_used/disk_free/disk_percent/disk_read_bytes/disk_write_bytes/net_sent_bytes/net_recv_bytes/uptime_s）
  - `service`：服务状态指标（active_sessions/history_sessions/finished_sessions/error_sessions/cancelled_sessions/total_sessions/recent_completed/avg_elapsed_s）
  - `sandbox`：沙盒状态（mode/network/readonly_rootfs/idle_ttl_s/timeout_s/endpoint/image/resources(cpu/memory_mb/pids)/recent_calls/recent_sessions）
  - `sessions`：活动线程列表（start_time/session_id/user_id/question/status/token_usage/elapsed_s/stage/summary）
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
  - `sessions`：调用会话列表（session_id/user_id/question/status/stage/start_time/updated_time/elapsed_s/token_usage/tool_calls/last_time）

### 4.1.9 `/wunder/admin/monitor/{session_id}`

- 方法：`GET`
- 返回（JSON）：
  - `session`：线程详情
  - `events`：事件详情列表
- 说明：
- 事件列表会按 `observability.monitor_event_limit` 保留最近 N 条，<= 0 表示不截断。
  - 字符串字段会按 `observability.monitor_payload_max_chars` 截断。
  - `llm_request` 事件仅保存 `payload_summary` 与 `message_count`，不保留完整请求体。
  - `observability.monitor_drop_event_types` 可过滤不持久化的事件类型。

### 4.1.10 `/wunder/admin/monitor/{session_id}/cancel`

- 方法：`POST`
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息

### 4.1.11 `/wunder/admin/monitor/{session_id}`

- 方法：`DELETE`
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息

### 4.1.12 `/wunder/workspace`

- 方法：`GET`
- 入参（Query）：
  - `user_id`：用户唯一标识
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
  - `path`：相对路径（文件）
- 返回：文件流

### 4.1.17 `/wunder/workspace/archive`

- 方法：`GET`
- 入参（Query）：
  - `user_id`：用户唯一标识
  - `path`：相对路径（可选，目录/文件；留空则全量打包）
- 返回：工作区全量或指定目录的压缩包文件流

### 4.1.18 `/wunder/workspace`

- 方法：`DELETE`
- 入参（Query）：
  - `user_id`：用户唯一标识
  - `path`：相对路径（文件或目录）
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
  - `tree_version`：工作区树版本号

### 4.1.19 `/wunder/workspace/dir`

- 方法：`POST`
- 入参（JSON）：
  - `user_id`：用户唯一标识
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
  - `path`：文件相对路径
  - `content`：文件内容
  - `create_if_missing`：文件不存在时是否创建（默认 false）
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
  - `tree_version`：工作区树版本号
  - `files`：保存的文件路径

### 4.1.24 `/wunder/web`

- 方法：`GET`
- 说明：提供前端调试页面与静态资源（`web/` 目录），用于远程访问调试。

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
  - `knowledge`：知识库配置（bases 数组，元素包含 name/description/root/enabled）
- `POST` 入参：
  - `knowledge`：完整知识库配置，用于保存与下发
  - 说明：当 root 为空时，服务端会自动创建 `./knowledge/<知识库名称>` 目录并回填配置

### 4.1.27 `/wunder/admin/knowledge/files`

- 方法：`GET`
- 入参（Query）：
  - `base`：知识库名称
- 返回（JSON）：
  - `base`：知识库名称
  - `files`：Markdown 文件相对路径列表

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

### 4.1.29 `/wunder/admin/knowledge/upload`

- 方法：`POST`
- 入参（multipart/form-data）：
  - `base`：知识库名称
  - `file`：待上传文件
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
  - `path`：转换后的 Markdown 相对路径
  - `converter`：使用的转换器（doc2md/python-xxx）
  - `warnings`：转换警告列表
- 说明：支持 doc2md README 中列出的扩展名；若 doc2md 不可用或执行失败，将尝试使用 Python 库进行兜底转换。

### 4.1.30 `/wunder/admin/knowledge/refresh`

- 方法：`POST`
- 入参（Query）：
  - `base`：知识库名称（可选，留空则刷新全部）
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息

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

### 4.1.34 `/wunder/admin/memory/users`

- 方法：`GET`
- 返回（JSON）：
  - `users`：长期记忆用户列表
    - `user_id`：用户标识
    - `enabled`：是否启用长期记忆
    - `record_count`：记忆记录数量
    - `last_updated_time`：最近更新时间（ISO）
    - `last_updated_time_ts`：最近更新时间戳（秒）

### 4.1.35 `/wunder/admin/memory/status`

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

### 4.1.36 `/wunder/admin/memory/status/{task_id}`

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

### 4.1.37 `/wunder/admin/memory/{user_id}`

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

### 4.1.38 `/wunder/admin/memory/{user_id}/{session_id}`

- 方法：`PUT`
- 入参（JSON）：
  - `summary`：记忆内容（纯文本段落）
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息

### 4.1.39 `/wunder/admin/memory/{user_id}/enabled`

- 方法：`POST`
- 入参（JSON）：
  - `enabled`：是否启用长期记忆
- 返回（JSON）：
  - `user_id`：用户标识
  - `enabled`：是否启用长期记忆

### 4.1.40 `/wunder/admin/memory/{user_id}/{session_id}`

- 方法：`DELETE`
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
  - `deleted`：删除条数

### 4.1.41 `/wunder/admin/memory/{user_id}`

- 方法：`DELETE`
- 返回（JSON）：
  - `ok`：是否成功
  - `message`：提示信息
  - `deleted`：删除条数

### 4.2 流式响应（SSE）

- 响应类型：`text/event-stream`
- `event: progress`：阶段性过程信息（摘要）
- `event: llm_request`：模型 API 请求体（调试用；监控持久化会裁剪为 `payload_summary`，若上一轮包含思考过程，将在 messages 中附带 `reasoning_content`）
- `event: knowledge_request`：知识库检索模型请求体（调试用）
- `event: llm_output_delta`：模型流式增量片段（调试用，`data.delta` 为正文增量，`data.reasoning_delta` 为思考增量，需按顺序拼接）
- `event: llm_stream_retry`：流式断线重连提示（`data.attempt/max_attempts/delay_s` 说明重连进度，`data.will_retry=false` 或 `data.final=true` 表示已停止重连，`data.reset_output=true` 表示应清理已拼接的输出）
- `event: llm_output`：模型原始输出内容（调试用，`data.content` 为正文，`data.reasoning` 为思考过程，流式模式下为完整聚合结果）
- `event: token_usage`：单轮 token 统计（input/output/total）
- `event: tool_call`：工具调用信息（名称、参数）
- `event: tool_result`：工具执行结果
- `event: compaction`：上下文压缩信息（原因/阈值/重置策略/执行状态）
- `event: final`：最终回复
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

### 4.3 非流式响应

- 返回 JSON：
  - `session_id`
  - `answer`
  - `usage`（可选）

### 4.4 工具协议（EVA 风格）

- 模型以 `<tool_call>...</tool_call>` 包裹 JSON 调用工具。
- JSON 结构：`{"name":"工具名","arguments":{...}}`。
- 工具结果以 `tool` 角色回填给模型，用于下一轮判断。
- 命令执行是否受限由 `security.allow_commands` 控制，支持 `*` 放开全部命令。
- 执行命令支持 `workdir` 指定工作目录（工作区或白名单目录），`shell` 仅在 allow_commands 为 `*` 时启用且默认开启，可显式传 `shell=false` 关闭，`timeout_s` 可选。
- 文件类内置工具默认仅允许访问工作区，可通过 `security.allow_paths` 放行白名单目录（允许绝对路径）。
- MCP 工具调用形式为 `server@tool`，技能工具按管理员启用的名称暴露。

示例：

```text
<tool_call>
{"name":"列出文件","arguments":{"path":"."}}
</tool_call>
```

### 4.5 存储说明

- 系统日志、对话历史、工具日志、产物索引、监控记录、会话锁与溢出事件统一写入 SQLite（配置项 `storage.db_path`）。
- 旧版 `data/historys/` 仅用于迁移与兼容，不再作为主存储。

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
  - `image`：字符串，沙盒镜像
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

## 5. 附录：辅助脚本

- `scripts/update_feature_log.py`：写入 `docs/功能迭代.md` 的辅助脚本，默认使用 UTF-8 BOM 避免乱码。

