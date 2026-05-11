# 外部工作流 API 方案

## 1. 目标

外部工作流 API 用于把 Wunder 的单个智能体嵌入到团队前端小部件中。外部系统提交一次任务后，Wunder 负责找到指定用户与指定智能体，打断该智能体当前正在进行的工作，强制开启新的会话，在 10 号工作目录中准备输入文件并执行任务。执行期间，API 需要持续返回模型中间动作、工具调用、工具结果、审批等待、工作区变更和最终消息；任务结束后还要返回最终引用文件，并提供可下载入口。

本方案先定义后端能力边界和协议细节，后续实现时再同步更新 `docs/API文档.md` 与使用说明书。

## 2. 非目标

- 不把该能力做成蜂群任务。现有 `team_runs` 面向多智能体任务编队，本方案面向单用户、单智能体、单次外部嵌入调用。
- 不通过临时修改智能体的 `sandbox_container_id` 实现 10 号目录切换。智能体配置是持久状态，直接改会影响其它并发会话。
- 不复用普通聊天页的 UI 协议作为外部 API 契约。外部 API 可以复用内部编排流，但对外输出要有稳定的 run 级协议。
- 不让外部系统直接下载任意工作区路径。下载必须绑定到本次 run 的文件清单。
- 不在 MVP 阶段支持同一用户多个外部工作流并发共享 10 号目录。10 号目录会在任务前清空，因此必须先建立互斥边界。

## 3. 核心概念

- 外部工作流 run：外部系统发起的一次性任务，主键为 `run_id`。
- 目标用户：外部请求中指定的用户名或用户标识，解析为 Wunder 内部 `user_id`。
- 目标智能体：外部请求中指定的智能体名，解析为该用户可访问的 `agent_id`。
- 工作会话：本次 run 强制新建的聊天会话，主键为 `session_id`。
- 10 号工作目录：`container_id=10` 对应的用户工作区，仅作为外部工作流临时目录使用。
- 输入文件：外部请求上传的文件，写入 10 号目录的 `input/` 子目录。
- 结果文件：最终消息引用到的文件，以及本次 run 生成且被结果清单收录的文件。
- 事件流：执行期间产生的结构化事件，包括工具调用、工具结果、模型输出、工作区变更、终态等。

## 4. 总体链路

```text
外部前端
  -> 创建外部工作流 run
  -> Wunder 鉴权与解析用户/智能体
  -> 打断目标智能体当前工作
  -> 获取用户 10 号目录互斥锁
  -> 清空 10 号目录
  -> 写入输入文件
  -> 强制新建会话并设为该智能体主线程
  -> 以请求级 workspace_container_id=10 调用编排器
  -> 流式转发工具与模型事件
  -> 收敛最终消息
  -> 收集引用文件并生成 file_id
  -> 返回最终结果与下载链接
```

## 5. API 入口

### 5.1 创建并流式执行

推荐提供 SSE 入口：

```http
POST /wunder/external/workflows:stream
Authorization: Bearer <external_or_user_token>
Content-Type: multipart/form-data
Accept: text/event-stream
```

字段：

- `request`：JSON 字符串，描述用户、智能体、消息与策略。
- `files[]`：可重复文件字段，作为输入附件。

`request` 示例结构：

```json
{
  "user_name": "<user_name>",
  "agent_name": "<agent_name>",
  "message": "<message>",
  "preempt": true,
  "workspace_container_id": 10,
  "clear_workspace": true,
  "timeout_s": 600,
  "client_run_id": "<optional_idempotency_key>",
  "metadata": {
    "source": "external_widget"
  }
}
```

说明：

- `workspace_container_id` MVP 固定为 `10`，外部传其它值直接拒绝，避免把该 API 泛化为任意工作区执行入口。
- `clear_workspace` MVP 固定为 `true`，因为需求明确要求清空 10 号目录。
- `preempt=true` 表示允许 Wunder 打断目标智能体当前工作。MVP 默认必须为 true；传 false 时如果目标智能体忙碌则返回 `409`。
- `client_run_id` 用于外部系统幂等重试。相同用户、智能体、`client_run_id` 在短时间内重复提交时返回同一 `run_id` 或拒绝重复执行。

SSE 事件格式：

```text
event: workflow.event
id: 12
data: {"run_id":"run_xxx","session_id":"sess_xxx","type":"tool_call","data":{}}
```

终态事件：

```text
event: workflow.final
id: 98
data: {"run_id":"run_xxx","status":"completed","answer":"...","files":[]}
```

### 5.2 创建异步 run

为不方便保持 SSE 连接的外部系统提供异步入口：

```http
POST /wunder/external/workflows
Authorization: Bearer <external_or_user_token>
Content-Type: multipart/form-data
```

返回：

```json
{
  "data": {
    "run_id": "run_xxx",
    "session_id": "sess_xxx",
    "status": "queued",
    "events_url": "/wunder/external/workflows/run_xxx/events",
    "cancel_url": "/wunder/external/workflows/run_xxx/cancel"
  }
}
```

MVP 可以先实现流式入口，异步入口在同一套 run 记录上复用。

### 5.3 查询状态

```http
GET /wunder/external/workflows/{run_id}
Authorization: Bearer <external_or_user_token>
```

返回：

```json
{
  "data": {
    "run_id": "run_xxx",
    "session_id": "sess_xxx",
    "user_id": "user_xxx",
    "agent_id": "agent_xxx",
    "status": "completed",
    "answer": "...",
    "stop_reason": "final_tool",
    "usage": {},
    "files": [],
    "created_at": 0,
    "started_at": 0,
    "finished_at": 0,
    "error": null
  }
}
```

### 5.4 拉取事件

```http
GET /wunder/external/workflows/{run_id}/events?after_event_id=0&limit=200
Authorization: Bearer <external_or_user_token>
```

说明：

- 用于断线恢复或异步模式轮询。
- 返回 run 级事件，不要求外部系统理解内部 `stream_events` 表结构。
- 事件需要按 `event_id` 单调递增，支持去重与续传。

### 5.5 终止 run

```http
POST /wunder/external/workflows/{run_id}/cancel
Authorization: Bearer <external_or_user_token>
```

返回：

```json
{
  "data": {
    "run_id": "run_xxx",
    "session_id": "sess_xxx",
    "status": "cancelling",
    "cancel_requested": true
  }
}
```

### 5.6 下载文件

```http
GET /wunder/external/workflows/{run_id}/files/{file_id}
Authorization: Bearer <external_or_user_token>
```

说明：

- `file_id` 只能来自该 run 的结果文件清单。
- 下载接口内部映射到 10 号目录真实路径，不向外暴露任意路径读取能力。
- 响应使用 `Content-Disposition: attachment`。

## 6. 运行节点

### 6.1 节点 A：鉴权与请求校验

输入：

- 请求头 token。
- `request` JSON。
- 可选文件。

处理：

- 校验 token。优先支持现有用户 token；外部系统专用 token 可复用 `security.external_auth_key` 或新增外部工作流 key。
- 校验 `user_name`、`agent_name`、`message` 至少满足消息或文件非空。
- 校验文件数量、单文件大小、总大小、文件名合法性。
- 校验 `workspace_container_id=10` 和 `clear_workspace=true`。

失败：

- `401 AUTH_REQUIRED`
- `403 PERMISSION_DENIED`
- `400 INVALID_REQUEST`
- `413 PAYLOAD_TOO_LARGE`

### 6.2 节点 B：解析用户与智能体

处理：

- 将 `user_name` 解析为内部 `user_id`。如果需要兼容虚拟用户，要明确虚拟用户只用于工作区与线程隔离，不自动获得管理权限。
- 在该用户可访问范围内按 `agent_name` 解析 `agent_id`。
- 如果名称不唯一，返回 `409 AGENT_NAME_AMBIGUOUS`。
- 如果未找到，返回 `404 AGENT_NOT_FOUND`。

输出：

- `user_id`
- `agent_id`
- 智能体配置快照，包括模型、工具、审批模式、系统提示词、预览技能标记。

### 6.3 节点 C：建立 run 记录

建议 MVP 复用 `session_runs`：

- `run_kind = "external_workflow"`
- `requested_by = "external_workflow_api"`
- `dispatch_id = client_run_id 或 run_id`
- `metadata` 保存外部请求摘要、容器号、文件数量、策略字段。

如果后续需要更强的文件治理，再新增 `external_workflow_runs` 与 `external_workflow_files` 表。

状态初始为 `preparing`。

### 6.4 节点 D：打断目标智能体当前工作

处理顺序：

1. 查找 `user_id + agent_id` 当前主线程。
2. 如果主线程有正在运行或等待的会话，调用监控取消能力，将状态置为 `cancelling`。
3. 清理该线程下 `pending/running/retry` 的队列任务。
4. 取消待处理审批。
5. 写入 run 事件 `preempt_requested`。

注意：

- 打断是协作式取消。正在执行的工具需要在工具边界检查取消信号，不能保证立即杀死所有外部进程。
- 新 run 不应复用旧会话，必须继续走“新建会话”节点。

### 6.5 节点 E：获取 10 号目录互斥锁

因为 10 号目录会被清空，所以必须防止同一用户多个外部工作流并发写同一个目录。

MVP 策略：

- 互斥粒度：`user_id + container_id=10`。
- 同一用户已有外部工作流运行时，默认返回 `409 EXTERNAL_WORKFLOW_BUSY`。
- 如果请求显式 `preempt_active_workflow=true`，则先取消该用户 10 号目录上活跃的外部工作流，再进入清空流程。

后续如需要并行，应改为 `container 10 / external_runs/{run_id}` 隔离目录，并取消“清空整个 10 号目录”的语义。

### 6.6 节点 F：清空 10 号目录

处理：

- 通过 `workspace.scoped_user_id_by_container(user_id, 10)` 得到工作区 id。
- `ensure_user_root` 创建根目录。
- 安全清空根目录内容。
- 清理工作区树缓存、搜索缓存、版本标记。
- 写入 run 事件 `workspace_cleared`。

约束：

- 只能清空解析后的 10 号用户工作区根目录。
- 清空前必须校验目标路径位于工作区根目录内。
- 不允许接收外部传入的任意清空路径。

### 6.7 节点 G：写入输入文件

目录约定：

```text
/
  input/
    <uploaded files>
  output/
    <agent generated files>
```

处理：

- 所有上传文件写入 `input/`。
- 文件名只保留 basename，拒绝 `..`、绝对路径、控制字符和保留名。
- 同名文件按策略覆盖或追加序号，MVP 建议追加序号。
- 将输入文件转换为 `AttachmentPayload.public_path` 或在消息中显式说明输入路径。

消息注入建议：

```text
<message>

输入文件位于：
- input/<file_name>
```

这里的路径必须是工作区相对路径，避免把服务器本地绝对路径暴露给模型与外部系统。

### 6.8 节点 H：强制新建会话

处理：

- 创建新 `session_id`。
- 将该会话设为 `user_id + agent_id` 的主线程。
- 写入会话记录，标题可由消息前缀生成。
- 冻结该会话工具配置与系统提示词。
- run 状态进入 `running`。

注意：

- 新会话是本次 run 的事实边界。
- 不应把外部 run 的消息追加到旧主线程。

### 6.9 节点 I：请求级切换到 10 号目录

需要给内部 `WunderRequest` 增加请求级字段：

```rust
workspace_container_id: Option<i32>
```

编排器解析工作区时优先级：

1. `request.workspace_container_id`
2. 智能体 `sandbox_container_id`
3. 默认容器

外部工作流入口固定传入 `Some(10)`。

原因：

- 不能修改智能体持久配置。
- 同一智能体普通聊天仍应使用它原本的目录。
- 外部 run 的系统提示词必须看到 10 号目录对应的工作区路径。

### 6.10 节点 J：执行编排器并转发事件

内部调用：

- 使用 `orchestrator.stream(request)`。
- `request.stream=true`。
- `request.allow_queue=false` 或受配置控制。MVP 建议外部工作流不进入普通聊天队列，容量不足时返回 `429`，避免队列等待期间 10 号目录被其它请求清空。

转发事件：

- `progress`
- `llm_request`
- `llm_response`
- `llm_output_delta`
- `tool_call`
- `tool_result`
- `approval_request`
- `approval_result`
- `workspace_update`
- `round_usage`
- `final`
- `turn_terminal`
- `error`

事件转换规则：

```json
{
  "run_id": "run_xxx",
  "session_id": "sess_xxx",
  "event_id": 12,
  "type": "tool_call",
  "created_at": "2026-05-11T00:00:00+08:00",
  "data": {}
}
```

对外字段要求：

- 必须包含 `run_id` 和 `session_id`。
- 必须保留 `tool_call_id`，方便外部前端把工具调用和结果归并到同一个步骤。
- 工具参数与结果需要做密钥脱敏。
- 大输出按现有工具结果裁剪策略返回，不在事件中塞完整大文件内容。

### 6.11 节点 K：收敛最终消息

终态来源：

- `final` 事件。
- `turn_terminal` 事件。
- 编排器返回的 `WunderResponse`。
- 错误或取消事件。

完成时写入：

- `answer`
- `stop_reason`
- `usage`
- `status`
- `finished_at`
- `elapsed_s`

如果没有最终消息但 run 已终止：

- `cancelled`：返回空 `answer`，状态为 `cancelled`。
- `failed`：返回错误码与错误消息。
- `timeout`：返回超时状态并保留已收集事件。

### 6.12 节点 L：收集结果文件

文件来源按优先级收集：

1. 最终消息中的工作区路径引用，例如 `/workspaces/...`、`workspace/...`、相对路径。
2. `workspace_update.changed_paths`。
3. `artifact_logs` 中 `kind=file` 的写入或补丁记录。
4. `output/` 目录下本次 run 生成的文件。

收集规则：

- 只收录 10 号目录内真实存在的文件。
- 默认优先返回“最终消息明确引用”的文件。
- 可额外返回 `generated_files`，但要与 `referenced_files` 分开，避免外部前端误以为都是模型最终引用。
- 文件大小超过限制时不内联，只提供下载。
- 同一路径只生成一个 `file_id`。

文件清单结构：

```json
{
  "file_id": "file_xxx",
  "name": "result.txt",
  "path": "output/result.txt",
  "kind": "referenced",
  "mime_type": "text/plain",
  "size": 123,
  "download_url": "/wunder/external/workflows/run_xxx/files/file_xxx"
}
```

### 6.13 节点 M：释放锁与清理

处理：

- run 到达终态后释放 `user_id + container_id=10` 锁。
- 保留 10 号目录内容，直到下一次外部工作流开始时清空。
- 保留 run 记录、事件记录、文件映射，便于外部系统补拉。
- 可配置 TTL 后清理旧 run 元数据。

## 7. 状态机

```text
preparing
  -> resolving
  -> preempting
  -> workspace_locked
  -> workspace_clearing
  -> uploading
  -> session_creating
  -> running
  -> completed

running
  -> cancelling
  -> cancelled

任何非终态
  -> failed
  -> timeout
```

状态定义：

- `preparing`：已收到请求，尚未解析完。
- `resolving`：正在解析用户与智能体。
- `preempting`：正在打断目标智能体旧任务。
- `workspace_locked`：已获得 10 号目录互斥锁。
- `workspace_clearing`：正在清空 10 号目录。
- `uploading`：正在写入输入文件。
- `session_creating`：正在新建会话并设置主线程。
- `running`：编排器正在执行。
- `cancelling`：收到终止请求，正在协作式取消。
- `completed`：正常完成。
- `failed`：失败终止。
- `cancelled`：取消完成。
- `timeout`：超时终止。

终态：`completed/failed/cancelled/timeout`。

## 8. 取消能力

取消入口必须做三件事：

1. 将 run 状态置为 `cancelling`。
2. 对 `session_id` 调用监控取消能力。
3. 清理该会话所属线程下仍在 `pending/running/retry` 的队列任务。

同时要处理：

- 待审批请求：自动拒绝并发送 `approval_resolved`。
- SSE 连接：发送 `workflow.cancelled` 后关闭。
- 正在运行的工具：通过现有取消检查在工具边界退出；无法立即停止的外部进程要在工具结果中标记 `cancel_requested`。

取消不是删除。取消后仍允许查询事件、最终状态和已生成文件。

## 9. 鉴权与权限

推荐两种模式：

### 9.1 用户 token 模式

外部前端先通过现有外部登录换取用户 token，再调用外部工作流 API。

优点：

- 权限天然落在用户边界内。
- 可复用现有用户配额、智能体访问控制和审计。

### 9.2 服务端 key 模式

外部系统后端持有 `external_workflow_key`，请求中声明 `user_name` 与 `agent_name`。

要求：

- key 只允许访问外部工作流 API。
- 必须记录调用方、目标用户、目标智能体、来源 IP、文件数量、token 使用量。
- 默认不授予管理员权限。

MVP 可先复用 `security.external_auth_key`，但代码中要把权限语义命名为 external workflow scope，避免和外链登录混淆。

## 10. 并发策略

MVP 约束：

- 同一 `user_id + container_id=10` 同时只允许一个 active 外部工作流。
- 新 run 创建时会打断同一目标智能体的普通工作。
- 新 run 默认不打断同一用户其它智能体的普通工作，但如果其它外部工作流占用 10 号目录，则返回冲突或按策略取消。

原因：

- 10 号目录会整体清空。
- 如果允许并发，不同 run 的输入和输出会互相污染。

后续并发扩展：

- 改为 `external_runs/{run_id}/` 作为实际工作根。
- 系统提示词注入当前 run 子目录。
- 清理策略从“清空整个 10 号目录”改为“清空当前 run 子目录”。

## 11. 文件安全

上传限制：

- 单文件大小：默认 50 MB，可配置。
- 总大小：默认 200 MB，可配置。
- 文件数量：默认 20 个，可配置。
- 文件名只允许安全 basename。
- 压缩包解压必须防 zip slip，且有总大小与文件数限制。

下载限制：

- 只能下载 run 文件清单里的 `file_id`。
- `file_id` 映射必须校验路径仍在 10 号目录内。
- 不允许下载隐藏的系统元数据文件。
- 响应头必须设置安全的文件名。

事件限制：

- 工具结果中不直接内联大文件内容。
- 工具参数与环境变量类字段要脱敏。
- 错误详情可给外部前端展示，但内部堆栈只进服务端日志。

## 12. 错误码

- `AUTH_REQUIRED`：缺少或无效鉴权。
- `PERMISSION_DENIED`：调用方无权访问用户或智能体。
- `USER_NOT_FOUND`：目标用户不存在。
- `AGENT_NOT_FOUND`：目标智能体不存在。
- `AGENT_NAME_AMBIGUOUS`：智能体名不唯一。
- `EXTERNAL_WORKFLOW_BUSY`：10 号目录已有活跃外部工作流。
- `PREEMPT_REQUIRED`：目标智能体忙碌但请求不允许打断。
- `WORKSPACE_LOCK_TIMEOUT`：获取 10 号目录锁超时。
- `WORKSPACE_CLEAR_FAILED`：清空目录失败。
- `UPLOAD_FAILED`：输入文件写入失败。
- `RUN_TIMEOUT`：执行超时。
- `RUN_CANCELLED`：任务被取消。
- `FILE_NOT_FOUND`：文件不存在或不属于该 run。

## 13. 数据模型建议

### 13.1 MVP：复用 session_runs

字段映射：

- `run_id`：外部工作流 run id。
- `session_id`：新建工作会话。
- `parent_session_id`：被打断的旧主会话，可为空。
- `user_id`：目标用户。
- `agent_id`：目标智能体。
- `run_kind`：`external_workflow`。
- `requested_by`：`external_workflow_api`。
- `dispatch_id`：外部 `client_run_id` 或内部 `run_id`。
- `status`：状态机状态。
- `result`：最终消息、文件清单、usage。
- `error`：错误信息。
- `metadata`：容器号、来源、输入文件摘要、策略。

优点：

- 现有 SQLite/Postgres 都已有 `session_runs`。
- 管理端与蜂群已有部分 run 查询经验可复用。

### 13.2 扩展：新增文件映射表

如需要稳定下载和 TTL 清理，新增 `external_workflow_files`：

- `file_id`
- `run_id`
- `user_id`
- `session_id`
- `workspace_id`
- `relative_path`
- `name`
- `mime_type`
- `size`
- `kind`
- `created_at`
- `expires_at`

## 14. 实现步骤

### 第一步：协议与数据骨架

- 新增 `src/api/external_workflows.rs`。
- 注册路由。
- 定义请求、事件、状态、文件清单结构。
- 复用 `session_runs` 写入 run 状态。
- 增加基础鉴权与用户/智能体解析。

### 第二步：请求级 10 号目录覆盖

- 给 `WunderRequest` 增加 `workspace_container_id`。
- 编排器解析工作区时优先使用请求级容器。
- 外部工作流固定传 `10`。
- 补测试确认不会修改智能体持久 `sandbox_container_id`。

### 第三步：10 号目录清空与上传

- 给 `WorkspaceManager` 增加安全清空指定用户容器的方法。
- 增加 `user_id + container_id` 互斥锁。
- 实现 multipart 文件落盘到 `input/`。
- 写入 `workspace_cleared` 与 `input_files_ready` 事件。

### 第四步：打断与新会话

- 找到目标智能体当前主线程。
- 取消旧会话 monitor 与队列任务。
- 新建会话并设为主线程。
- 构造 `WunderRequest` 并调用 `orchestrator.stream()`。

### 第五步：事件转发与断线补偿

- 监听内部 stream event。
- 过滤和规范化对外事件。
- 写入 run 事件记录。
- 实现 `GET /events` 补拉。

### 第六步：最终文件收集与下载

- 解析最终消息中的文件引用。
- 合并 `workspace_update` 和 artifact logs。
- 生成 `file_id` 映射。
- 实现下载接口。

### 第七步：取消、超时与测试

- 实现 cancel endpoint。
- 加 run 级 timeout。
- 补齐 SQLite/Postgres 相关测试。
- 覆盖取消、目录清空、文件下载、事件续传、并发冲突。

## 15. 测试清单

- 用户名能解析到正确用户。
- 智能体名能解析到正确智能体；重名时返回冲突。
- 创建 run 前会打断同一智能体旧会话。
- 创建 run 会强制新建会话并设为该智能体主线程。
- 请求级 `workspace_container_id=10` 生效，且不修改智能体配置。
- 10 号目录清空只影响目标用户的 10 号目录。
- 并发请求同一用户 10 号目录时返回冲突或按策略取消旧 run。
- 上传文件只落在 `input/` 内，非法路径被拒绝。
- SSE 能返回 `tool_call/tool_result/final/error`。
- `GET /events` 能按 `after_event_id` 补拉。
- cancel 能让 run 进入 `cancelled`，并保留事件。
- 最终消息引用文件会进入 `files[]`。
- 下载接口只能下载该 run 的 `file_id`。
- 超时会进入 `timeout`，并释放互斥锁。

## 16. 关键风险与处理

- 风险：清空 10 号目录会影响并发 run。
  处理：MVP 强制 `user_id + container_id=10` 单活。

- 风险：修改智能体目录配置造成其它会话串目录。
  处理：只使用请求级 `workspace_container_id`，不改智能体记录。

- 风险：取消无法立即停止长时间外部命令。
  处理：先保证 monitor 与队列状态收敛；工具层逐步补强取消检查和子进程终止。

- 风险：外部前端展示工具参数导致敏感信息泄露。
  处理：对外事件统一走脱敏函数，默认隐藏 token、key、secret、authorization 等字段。

- 风险：最终文件识别不完整。
  处理：先返回明确引用文件；同时提供 `generated_files` 作为补充，不混淆语义。

- 风险：外部系统重复提交导致重复执行。
  处理：支持 `client_run_id` 幂等键。

## 17. MVP 验收标准

- 外部系统可以用一次请求指定用户、智能体、消息和文件。
- Wunder 会打断目标智能体当前运行，创建新会话执行任务。
- 任务执行期间可以实时看到工具调用和工具结果。
- 终止接口可以取消运行中的 run。
- 任务完成后返回最终消息。
- 最终消息引用到的文件能通过返回的下载链接下载。
- 同一用户 10 号目录不会被并发 run 互相污染。
