# 子 agent 落地方案（统一子智能体控制）

## 1. 目标与范围
- 让模型在同一会话内可调用“子 agent 会话”，形成可并行、可追踪、可回传的任务链路。
- 参考 OpenClaw 的 session 能力，但对外仅保留一个“子智能体控制”工具（`action=list/history/send/spawn`）。
- 保持 wunder 现有会话与监控体系不变形，仅补齐子会话关系与内部调度。

## 2. 现状评估（wunder）
- 已有 `chat_sessions`、`agent_id`、`tool_overrides`，并通过 orchestrator 执行单会话请求。
- 具备 MonitorState 监控与 SSE 事件持久化，可复用为子会话监控。
- 内置 a2a 工具面向外部服务，当前无“内部 spawn 子会话”能力。

## 3. 参考能力清单（OpenClaw）
- `list`：列出会话（可带最近消息）。
- `history`：拉取指定会话历史。
- `send`：跨会话投递消息，支持 `timeoutSeconds` 等待结果。
- `spawn`：异步启动子会话，立即返回 `runId + childSessionKey`；默认禁用子会话再 spawn；完成后进行 announce 回传。
- 子会话默认使用全工具集（不再单独暴露 session 工具），并支持 allowlist 指定 `agentId`。
- sandbox 会话默认只能看到“自己 spawn 的会话”。

## 4. 核心设计（wunder）

### 4.1 子智能体控制工具（统一入口）
新增内置工具（`src/services/tools.rs`）：`子智能体控制`
- `action=list`：列出当前用户可见会话（支持 `limit` / `activeMinutes` / `messageLimit`）。
- `action=history`：读取某会话历史（`session_id` 或兼容 `sessionKey`）。
- `action=send`：向指定会话发送消息；可选 `timeoutSeconds` 等待结果。
- `action=spawn`：创建子会话并异步运行，返回 `{status, run_id, child_session_id}`。

### 4.2 数据模型扩展
在 `chat_sessions` 上追加子会话关系字段（或建立独立关系表）：
- `parent_session_id`：父会话。
- `parent_message_id`：触发子会话的消息（可用于回溯）。
- `spawn_label`：前端展示标签。
- `spawned_by`：`user | model | tool`。

新增 `agent_runs` 或 `session_runs`（推荐）记录运行维度：
- `run_id / session_id / parent_session_id / user_id / agent_id / model`
- `status / queued_time / started_time / finished_time / elapsed_s`
- `result / error / updated_time`
- token/轮次可在后续补齐

### 4.3 子会话调度与回传
- `action=spawn` 创建子会话记录，立即 `tokio::spawn` 启动 orchestrator。
- 子会话完成后触发 **announce**：将结果写回父会话（追加 assistant 消息或写入 `workflow` 事件）。
- announce 文案统一为 `Status/Result/Notes` 结构；支持 `ANNOUNCE_SKIP` 静默。

### 4.4 跨会话“问答链路”对齐（可选）
对齐 OpenClaw 的 reply-back：
- `action=send` 在等待模式下可触发“父-子 ping-pong”轮次。
- 约定 `REPLY_SKIP` 终止回环；限制最大往返次数（0-5）。

## 5. 监控与可视化
- MonitorState 继续作为唯一来源，子会话也写入同一监控系统。
- 通过 `parent_session_id` 进行子会话聚合查询。
- 管理端 `/wunder/admin/monitor` 可直接看到子会话状态；详情通过 `/wunder/admin/monitor/{session_id}`。
- 可新增 `/wunder/chat/sessions/{id}/children` 便于前端展示子会话列表与状态。

## 6. 权限与安全
- `action=spawn` 目标 `agent_id` 受 allowlist 控制（默认仅自身 agent）。
- 子会话工具与主会话保持一致，不做子智能体控制工具剔除；递归调用由上层策略控制。
- sandbox 会话默认仅可见“自己 spawn 的子会话”（可配置为 all）。
- 子会话与父会话必须同一 `user_id`，管理员可跨用户查看（仅管理端）。

## 7. 资源与生命周期
- 运行限制：
  - `subagent_max_active`（与 `server.max_active_sessions` 并行约束）。
  - `runTimeoutSeconds`（子会话超时强制结束）。
- 生命周期：
  - 自动归档（例如 60 分钟后清理或标记归档）。
  - 历史裁剪遵循既有策略；管理员可不裁剪。
- Token 统计：以 `context_tokens` 为准，避免记录“总消耗”误差。

## 8. API / 前端协同
建议新增或复用：
- `GET /wunder/chat/sessions?parent_id=...`：列出子会话。
- `GET /wunder/chat/sessions/{id}/events`：子会话进度。
- `POST /wunder/chat/sessions/{id}/cancel`：支持取消子会话。

前端展示建议：
- 父会话内展示“子任务列表”，支持查看进度、查看结果、取消。
- 子会话完成后自动在父会话插入“摘要卡片”。

## 9. 实施节点（里程碑）

### 节点 A：数据模型与存储
- 增加子会话关系字段与运行表。
- 存储读写接口与索引补齐。
- 验收：可通过 `parent_session_id` 查询子会话。

### 节点 B：子智能体控制工具
- 内置 `子智能体控制` 工具（`action=list/history/send/spawn`）。
- 完成工具参数校验与权限过滤。
- 验收：模型能列出/查询/发送/生成子会话。

### 节点 C：子会话调度与 announce
- spawn 后异步执行，回传结果。
- announce 结构化输出与事件写入。
- 验收：父会话收到子会话结果与统计信息。

### 节点 D：权限与限制
- allowlist、最大深度、并发上限、sandbox 可见性。
- 验收：子会话不可再 spawn；越权禁止。

### 节点 E：前端展示 + 管理监控
- 子会话列表与状态面板。
- 管理端可筛选子会话与运行指标。
- 验收：可视化进度与可控取消。

### 节点 F：测试与文档
- 子会话全链路测试（spawn -> run -> announce）。
- 更新 API 文档与设计文档。

## 10. 差异与适配说明
- wunder 已具备 `agent_id` 与多会话，并已补齐子智能体控制工具与内部 spawn。
- OpenClaw 的多智能体 routing 属于网关层能力，wunder 暂以“会话内子 agent”对齐但不要求工具命名完全一致；后续可扩展 bindings 路由层。

---

该方案优先复用现有 orchestrator、monitor 与 chat/session 存储，补齐子智能体控制工具与调度，使功能可在原有架构中最小化接入，并逐步对齐 OpenClaw 的能力形态与子会话运行模型。


