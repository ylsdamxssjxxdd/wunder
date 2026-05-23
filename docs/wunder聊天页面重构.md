# wunder 聊天页面重构设计

## 上线收口状态（2026-05-23）

- 当前结论：聊天页主链路已切到 canonical runtime projection，刷新后历史顺序、停止后取消标记、同轮 assistant 合并和长输出节流已经形成一条可上线验证的闭环。
- 默认行为：Messenger 气泡渲染优先读取 `chatRuntimeReducer` 的可见消息投影；旧 `messages` 数组仅作为未知空投影会话的兜底和显式回退路径。
- 最近收口：失败事件与随后 `turn_terminal` 会合并到同一条 assistant 失败消息；错误正文保留但不会重复生成错误气泡。运行时工具名送入模型前统一清洗为 API 安全函数名，并保留映射回原始运行时名，避免外部渠道触发非法 function name 后重复渲染错误。
- 回退方式：线上如遇不可接受回归，可设置 URL 参数 `chat_runtime_render=legacy` / `chatRuntimeRender=off`，或 localStorage `wunder:chat-runtime-render=legacy/off/0/false`，快速退回旧渲染源；`shadow` 模式仍可用于只比较不切换。
- 本次验证：已通过 `npm run test:chat-runtime-reducer --workspace wunder-frontend`；Rust 侧已通过 `cargo test --release -j 8 prompt::tests::model_function_name_uses_readable_runtime_name_for_mcp_tools --lib -- --nocapture`。全量 `cargo test --release -j 8 model_function_name` 当前被无关测试中的 `UserAgentRecord.visible_unit_ids` 初始化缺字段阻断。
- 仍需上线观察：真实长输出、工具调用、审批、子智能体、多会话快速切换、微信/外部渠道错误回传和桌面弱机器场景需要灰度观察；一旦出现 projection/legacy drift，优先打开 shadow/debug 定位，再决定是否临时切 `legacy`。

## 结论

当前用户侧聊天问题不适合继续靠局部补丁修复。丢消息、重复消息、假死、消息抖动和实时性不稳定，本质上来自“同一条聊天时间线存在多个写入者”：发送流、watch 流、resume 流、HTTP 详情快照、HTTP 事件快照、本地乐观消息、旧 `messages` 数组、`runtimeProjection`、去重和 watchdog 都可能修改同一批消息。

重构目标是建立一条规范链路：后端生成可持久、可重放、可幂等的 canonical event log；前端只用一个会话订阅器消费事件；所有消息和运行态都进入一个 reducer/projection；渲染层只读取 projection 的派生结果，不再直接参与消息归并。

## 当前风险判断

### 前端多写入源

当前主要链路分散在以下文件：

- `frontend/src/stores/chatSendActions.ts`：发送消息、创建本地 user message、创建 assistant 占位、处理 request-scoped WS 事件。
- `frontend/src/stores/chatWatcher.ts`：watch session、恢复 pending message、watchdog、HTTP 事件快照、断线补偿。
- `frontend/src/stores/chatRuntimeState.ts`：全局 cache、snapshot、runtime projection 同步、消息数组去重。
- `frontend/src/stores/chatMessageDedup.ts`：基于相邻 assistant、内容长度、状态、时间等规则合并。
- `frontend/src/realtime/chat/chatRuntimeReducer.ts`：已有 projection 雏形，但当前仍被 legacy messages 反向喂入。
- `frontend/src/views/MessengerView.vue` 和 `frontend/src/views/messenger/controller/*`：仍以 `chatStore.messages` 为主要渲染源。

这会导致一个事件可能被多条路径处理：发送请求收到事件会改 assistant 占位；watch 也可能根据同一事件找 pending assistant；HTTP detail 又可能用服务端快照替换或合并本地消息；`cacheSessionMessages` 还会原地去重并改变数组。结果就是消息顺序、引用、key 和滚动高度都可能在短时间内变化，页面表现为抖动、重复、丢失或卡住。

### queued ack 后 request-scoped 事件易丢

`frontend/src/utils/ws.ts` 的 multiplexer 在 `resolveOnQueued` 为 true 且 `keepPendingAfterQueuedAck` 为 false 时，会在收到 queued ack 后从 `pending` 表移除 request。当前 `sendMessage` 调用使用了 `resolveOnQueued: true`、`closeOnFinal: true`，没有开启 `keepPendingAfterQueuedAck`。

这意味着 queued ack 后如果后端仍以同一个 request id 继续推送事件，multiplexer 会因为找不到 pending entry 而丢弃事件。当前系统依赖 watcher/resume 再捞回这些事件，这会引入实时延迟和前端归并竞争，是“偶发丢消息”和“页面状态卡住”的高风险点。

### 事件持久化与广播顺序不够确定

后端 `src/orchestrator/event_stream.rs` 的 `EventEmitter` 会为事件分配递增 `event_id`，并将事件发送到流队列。事件持久化走 `src/orchestrator/stream_persist.rs` 的异步队列，队列满时再 fallback 到阻塞写。

这个设计有性能优势，但 canonical 恢复语义不够强：前端可能已经收到某个事件，而紧接着 resume/watch 读取 `stream_events` 时，该事件还没有落库。`spawn_stream_pump` 有 gap recovery，但客户端侧 `resume_stream_events` 仍是轮询存储。对于稳定聊天，需要区分“实时广播”和“可恢复日志”的边界：凡是前端需要用来恢复、去重、终态结算的 canonical 事件，必须先获得稳定序号和可恢复写入保证，再对外广播。

### 渲染层依赖可变数组

当前 `messengerControllerRenderableMessages.ts` 从 `chatStore.messages` 直接 reduce 出 `agentRenderableMessages`，虚拟列表再基于这些对象引用、key 和高度缓存工作。只要消息数组被原地 splice、去重、替换引用或修改 key，虚拟窗口和滚动锚点就会受到影响。

因此性能问题不只是虚拟列表本身，而是上游数据不稳定。必须先让消息时间线稳定，再优化虚拟渲染。

## 重构目标

1. 消息不丢：断线、queued、慢客户端、切换会话后，最终 assistant 消息必须可由 event log 或 durable message 恢复。
2. 消息不重：同一 user turn/model turn 不因 send/watch/resume/snapshot 多路径重复生成 assistant。
3. 顺序稳定：流式输出期间消息 key、turn 归属和列表顺序不抖动。
4. 页面不卡：token 高频事件不触发整页级深度响应式更新；长历史列表使用稳定虚拟滚动。
5. 实时明确：发送请求只负责提交；订阅器负责接收；reducer 负责归并；渲染层只读 projection。
6. 可测试：后端事件序列、恢复、取消、queued、断线；前端 reducer 的乱序、重复、gap、snapshot reconcile 都有自动化测试。

## 非目标

- 不重写模型编排和工具执行核心逻辑。
- 不在本轮设计中改变桌面端 SQLite 与 server Postgres 的总体选型。
- 不继续扩大 `MessengerView.vue`。新功能应落到 `frontend/src/realtime/chat/`、`frontend/src/stores/`、`frontend/src/views/messenger/` 的新模块中。
- 不用内容相似度作为正常路径的消息归并依据。内容匹配只能作为迁移期兜底，不进入新协议主链路。

## 目标架构

### 后端作为唯一事实源

后端为每个会话 turn 生成规范事件，事件写入 `stream_events`，同时在 terminal 时写入 durable messages。前端可以乐观显示用户输入，但必须通过 server ack 绑定到服务端 `user_turn_id` 和 `message_id`。

规范事件必须包含：

- `event_id`：会话内可恢复递增 id，兼容现有 stream event offset。
- `event_seq`：turn 内或会话内严格连续序号，用于 reducer 检测 gap。
- `session_id`：会话 id。
- `request_id`：一次客户端提交 id。
- `user_turn_id`：用户轮次 id。
- `model_turn_id`：模型轮次 id。
- `message_id`：事件关联的消息 id。
- `event_type`：规范事件类型。
- `payload`：事件负载。
- `created_at`：服务端时间。

`event_id` 解决恢复起点，`event_seq` 解决乱序和 gap 检测，turn/message id 解决归属，避免前端靠 round、时间戳、内容长度猜测。

### 前端单一订阅器

前端每个活跃 session 只保留一个 canonical subscription。发送函数不再直接维护长生命周期流，也不再创建会被 watch 抢写的 assistant 占位。

新链路：

1. `sendMessage` 创建本地 client message，并提交 `start`。
2. 后端返回 `request_accepted` 或 `queue_accepted` ack，携带 `session_id`、`request_id`、`user_turn_id`、`client_message_id` 到 `message_id` 的映射。
3. 全局 session subscription 按 `after_event_id` 接收后续事件。
4. 所有事件进入 `chatRuntimeReducer`。
5. 渲染层读取 `selectVisibleMessageProjections` 或新的 selector。

发送请求可以等待 ack，但不能拥有后续事件的唯一处理权；watch/resume 也不能直接修改 `messages` 数组，只能投递 canonical events 或 snapshot events 到 reducer。

### Durable messages 与 live turn runtime 拆分

建议建立两个明确模型：

- durable timeline：已经由后端确认的 user/assistant/system messages，包含 message id、role、content、created_at、turn id、terminal status。
- live turn runtime：当前 model turn 的 streaming text、reasoning、工具 timeline、审批状态、子智能体状态、task board、busy reason。

流式 token 只更新 live runtime。终态事件 `assistant_final` 或服务端 `message_committed` 后，assistant 消息进入 durable timeline。UI 可以在同一位置展示 live assistant preview，但它的 key 必须是 `model_turn_id`，不能是内容或临时时间戳。

### Reducer 作为唯一归并点

`frontend/src/realtime/chat/chatRuntimeReducer.ts` 已经有较好的雏形：事件 id 去重、seq gap 检测、strict event quarantine、message/user turn/model turn projection。重构应把它提升为主链路，而不是继续通过 `legacy_messages_reconciled` 反向同步旧数组。

新 reducer 规则：

- 所有 WS、HTTP events、session snapshot 都转成统一 `ChatRuntimeEvent`。
- 重复 `event_id` 忽略。
- `event_seq` 小于等于已应用序号忽略。
- `event_seq` 出现小范围 gap 时先进入有界顺序缓冲，不立即应用后续 delta/final；缺口补齐后按序排空，避免后到的早期 delta 被 stale 规则吞掉。
- `event_seq` 超过有界缓冲能力或缓冲溢出时标记 `syncRequired` 并立即触发一次 events/snapshot 恢复；在后端尚未保证连续 canonical seq 前，前端仍兼容应用该事件，避免误伤合法跳号的历史链路。
- 没有 `session_id`、`event_id`、`event_seq`、turn id、message id 的严格事件进入 quarantine，不写消息。
- terminal event 只结算对应 `model_turn_id`，不扫描相邻 assistant。
- snapshot hydrate 只能补齐或覆盖 projection，不直接 splice 当前渲染数组。

## 后端 Rust 重构设计

### 1. 规范事件类型

新增 canonical chat event 层，建议落点：

- `src/orchestrator/chat_events.rs`：事件结构、类型枚举、payload builder。
- `src/orchestrator/chat_turn_state.rs`：turn state 快照结构与服务。
- `src/api/chat_ws.rs`：只做协议收发和订阅管理。
- `src/api/chat.rs`：补充事件 replay/snapshot API。
- `src/storage/postgres.rs`、`src/storage/sqlite.rs`：表结构与读写实现。

建议事件类型：

- `request_accepted`：请求已接收，返回 server turn/message id。
- `queue_accepted`：已入队，包含排队状态。
- `queue_started`：开始执行。
- `user_message_committed`：用户消息已落库。
- `assistant_message_started`：模型消息占位已由服务端确认。
- `assistant_delta`：文本增量。
- `assistant_reasoning_delta`：reasoning 增量。
- `tool_call_started`、`tool_call_delta`、`tool_call_completed`、`tool_call_failed`。
- `approval_requested`、`approval_resolved`。
- `subagent_*`：保持现有子智能体语义，但绑定 `model_turn_id`。
- `assistant_final`：最终 assistant 内容与 message id。
- `turn_completed`、`turn_failed`、`turn_cancelled`。
- `session_runtime`：运行态变更。
- `snapshot_available` 或 `sync_required`：提示客户端重放。

保留现有 `llm_output_delta`、`final`、`turn_terminal` 一段时间作为兼容层，但新前端只消费 canonical event。

### 2. 持久化顺序

对 canonical event 采用“分配序号 -> 持久化 -> 广播”的顺序。高频 delta 可以批量写 payload，但逻辑 event id/seq 必须稳定，恢复时能够按序重放。

建议拆分两类写入：

- boundary events：request、queue、message started/final、tool start/end、approval、terminal。必须同步或事务内持久化成功后广播。
- token delta events：允许内存批量和节流，但每个广播出去的 logical delta 必须可由 event log 恢复。可以把多个 token 合并为一个 `assistant_delta` event，再广播同一个已持久化 batch。

如果为了性能不能每 token 同步落库，则不要每 token 都作为 canonical event 广播；应在服务端按 30-80ms 或字符阈值聚合成 batch event。这样既减少前端渲染压力，也减少数据库写入压力。

### 3. 数据库表

Postgres/server 与 SQLite/desktop 保持同构字段，差异只在 SQL 方言。

建议新增或规范化：

`chat_turns`

- `turn_id`
- `session_id`
- `user_id`
- `request_id`
- `status`
- `created_at`
- `updated_at`
- `started_at`
- `finished_at`
- `last_event_id`
- `last_event_seq`

`chat_messages`

- `message_id`
- `session_id`
- `turn_id`
- `role`
- `content`
- `reasoning`
- `status`
- `client_message_id`
- `created_at`
- `updated_at`
- `meta`

现有 `chat_history` 可迁移或扩展，但新链路必须暴露稳定 `message_id` 和 `turn_id`。如果短期不迁移表名，也要在现有表中补齐这些字段。

`chat_turn_state`

- `session_id`
- `turn_id`
- `request_id`
- `lifecycle`
- `runtime_status`
- `streaming_text`
- `reasoning`
- `tool_timeline`
- `task_board`
- `last_event_id`
- `last_event_seq`
- `updated_at`

这个表是可覆盖快照，不是事实日志。正常完成后删除或标记 terminal；服务重启时把非 terminal 运行态标记为 interrupted/recovering。

`stream_events`

现有表继续使用，但需要确保字段包含或 payload 包含 `event_seq`、turn id、message id，并建立索引：

- `(session_id, event_id)`
- `(session_id, event_seq)`
- `(session_id, created_at)`

### 4. WebSocket 协议

将当前 `start`、`watch`、`resume` 三种会写消息的模型改成：

- `connect`：握手。
- `subscribe`：订阅一个 session，参数 `after_event_id` 和可选 `snapshot_seq`。
- `unsubscribe`：取消订阅。
- `start`：提交用户消息，只返回 ack 事件，不独占后续流。
- `cancel`：取消 turn。

服务端对订阅连接维护 session subscriber。新事件产生后广播给所有订阅者；客户端断线后用 `subscribe(after_event_id)` 继续。`watch` 和 `resume` 可以作为旧协议别名，但内部都转成 subscribe。

### 5. HTTP 恢复 API

保留并规范化：

- `GET /chat/sessions/:id/events?after_event_id=...&limit=...`
- `GET /chat/sessions/:id/snapshot`

events API 返回 canonical events，并包含：

- `events`
- `last_event_id`
- `last_event_seq`
- `has_more`
- `snapshot_required`

snapshot API 返回 durable messages、live turn state、last event cursor。前端收到 snapshot 后仍然通过 reducer hydrate，不直接替换渲染数组。

### 6. 取消、错误和终态

所有非成功结束都必须产生 terminal event：

- 用户取消：`turn_cancelled`
- 模型/工具错误：`turn_failed`
- 队列拒绝：`turn_failed` 或 `queue_failed`
- 服务关闭线程：`session_runtime` + `turn_failed`

terminal event 必须携带 `model_turn_id` 和最终 runtime status。前端只根据 terminal event 释放 composer 锁，不再依赖 socket close 或 HTTP detail 猜测。

## 前端重构设计

### 1. 新 store 模块

建议新增：

- `frontend/src/realtime/chat/chatCanonicalEvents.ts`：后端事件到前端 `ChatRuntimeEvent` 的转换。
- `frontend/src/realtime/chat/chatTimelineStore.ts`：Pinia store，持有 projection、cursor、subscription 状态。
- `frontend/src/realtime/chat/chatSubscription.ts`：单 session WS 订阅器。
- `frontend/src/realtime/chat/chatSnapshots.ts`：HTTP events/snapshot hydrate。
- `frontend/src/realtime/chat/chatTimelineSelectors.ts`：渲染 selector。

现有 `chatRuntimeReducer.ts` 可以保留并扩展，避免重新发明 reducer。

### 2. 发送链路

新 `sendMessage` 只做：

1. 生成 `client_message_id`。
2. dispatch `client_message_submitted` 到 reducer，显示本地 user bubble。
3. 通过 WS/HTTP 提交 start。
4. 收到 `request_accepted` 后绑定 `client_message_id -> message_id/user_turn_id`。
5. 确保当前 session subscription 正在运行。

发送函数不创建 assistant 占位；assistant preview 由 `assistant_message_started` 或首个 `assistant_delta` 创建，key 为 `model_turn_id`。

### 3. 订阅与恢复

每个活跃 session 只有一个 subscription controller。切换会话时：

- 旧 session 订阅可降级为后台轻量订阅或关闭。
- 新 session 从本地 cursor 的 `last_event_id` 订阅。
- 如果 reducer 标记 `syncRequired`，先拉 events，events 不完整再拉 snapshot。

不再允许 `sendController`、`watchController`、`resumeController` 同时写消息数组。旧的慢客户端恢复可以变成 subscription 内部的 backfill 行为：收到 `slow_client` 或发现 seq gap，就调用 events API 追平，然后继续订阅。

### 4. 消息归并规则

主链路只按 id 归并：

- user message：`client_message_id` 临时展示，server ack 后绑定 `message_id`。
- assistant message：`model_turn_id + message_id`。
- tool item：`tool_call_id`。
- subagent item：`task_id` 或服务端稳定 id。

禁止正常路径使用：

- assistant 相邻合并。
- 内容长度比较。
- 时间戳 1500ms 窗口。
- 中文乱码 token 判断。
- round 猜测作为唯一归属。

迁移期可以保留 legacy adapter，把旧事件补齐为 canonical event，但 adapter 输出缺字段时必须进入非 strict 模式，并打 debug 标记，不能污染新协议。

### 5. 渲染层

`MessengerView.vue` 和 controller 后续只读取 selector 输出：

- `visibleMessages`
- `liveAssistantByTurn`
- `toolTimelineByTurn`
- `sessionBusy`
- `composerState`

渲染对象应是稳定结构：

- `key` 永远来自 `message_id`、`client_message_id` 或 `model_turn_id`。
- 流式内容更新只改变对应 projection 的 `content`，不改变列表顺序。
- snapshot hydrate 不直接 splice 可见数组。
- 虚拟列表高度缓存按稳定 key 维护。

当 projection 主链路稳定后，再考虑把自研 `messageVirtualWindow` 替换为成熟 Vue 虚拟列表组件；如果继续自研，也必须以稳定 key、固定 overscan、尾部 pinning 和测量节流为基础。

### 6. 性能策略

- 后端按时间或字符聚合 delta，减少 WS 帧数量。
- 前端 subscription 收到 delta 后用 `requestAnimationFrame` 或 30-50ms flush 合并更新。
- legacy `chatWorkflowProcessor` 只能把正文与 reasoning delta 先累积在普通变量里，按 `streamFlushMs` + `requestAnimationFrame` 合并后再写 `assistantMessage.content/reasoning`；`final/error/cancel/stop` 这类边界事件必须 `flushStream(true)` 强制落最后一段。
- `notifySessionSnapshot` 需要用可见消息签名判断是否推进 `messageMutationVersion`、projection reconcile、窗口计算和快照落盘；纯轮次、内部 pending、无可见变化的流式 bookkeeping 不应触发整页重算。
- 工具 timeline 与正文 streaming 分开更新，避免一个 token 触发整个 workflow 面板重算。
- 长历史分页只进入 normalized store，不一次性深响应式挂载所有 raw message。
- Markdown 渲染对 final message 缓存 AST 或 HTML；streaming preview 使用轻量纯文本/增量渲染。
- 虚拟列表开启阈值提升到基于 DOM 成本和消息数的组合判断，不在短列表里引入虚拟滚动复杂性。

## 迁移节点

### 当前实施进度（2026-05-21）

- 已完成 canonical runtime projection 侧车：本地提交、send/watch/resume WS 事件、HTTP events snapshot 已统一投递到 `chatRuntimeReducer`，旧 `messages` 仍是当前 UI 渲染源。
- 已完成 request-scoped queued 保护：`queued` ack 只解析请求，不再立即丢弃后续同 request 事件。
- 已完成 events-first snapshot 基础：`GET /wunder/chat/sessions/{session_id}/events` 已返回原始 `events`，并为存储与 WS 事件补齐 `event_seq`。
- 已完成 shadow comparison 基础：新增 `frontend/src/realtime/chat/chatRuntimeShadow.ts`，在 chat debug 开启时对 canonical projection 与 legacy `messages` 做影子一致性检查，覆盖缺失、重复、顺序漂移、内容/reasoning/status 漂移和 busy 漂移。
- 已完成受控 selector render adapter：新增 `frontend/src/realtime/chat/chatRuntimeRenderAdapter.ts`，可通过 `wunder:chat-runtime-render` / `wunder_chat_runtime_render` 或 URL 参数 `chat_runtime_render` / `chatRuntimeRender` 显式把 Messenger 气泡渲染源切到 canonical projection；默认仍使用 legacy `messages`，projection 为空时自动回退。
- 已接入 Messenger 渲染侧车：`agentRenderableMessages` 可读取 projection 物化结果，稳定 key 来自 runtime message id；最新助手消息、实时 stats 也改为读取 renderable 列表，避免 projection 渲染开关开启后头像状态和 footer 继续读旧数组。
- 已新增 render-source shadow：`frontend/src/realtime/chat/chatRuntimeRenderShadow.ts` 比较 legacy renderable 与 projection renderable 的 key、缺失、顺序、内容、reasoning、streaming flag、workflow timeline 和 subagent 摘要；`wunder:chat-runtime-render=shadow` 或 `wunder:chat-runtime-render-shadow=1` 可在不切换真实 UI 的情况下输出差异。
- 已继续收敛 Messenger 直接读旧数组的渲染辅助：附件预加载、计划面板、询问面板、最新助手布局刷新、虚拟列表刷新触发、stats 上下文等开始跟随 `agentRenderableMessages`。
- 已推进展示派生层统一读 renderable source：`installMessengerControllerRenderableMessages` 提前到身份与导航状态之前安装，新增 `resolveActiveAgentRenderableMessageRecords()` 作为当前会话 UI 消息记录入口；顶部模型名、会话保留判断、忙碌判断/快照、会话预览刷新和发送后居中计数不再各自直接读取旧 `chatStore.messages`。新增 `test:messenger-renderable-source` 静态回归锁定安装顺序和展示读取源。
- 已完成 render-source key 统一：新增 `frontend/src/realtime/chat/chatRuntimeMessageKeys.ts`，投影渲染和 Messenger 路由偏好共用同一套稳定 key 解析，优先使用 `__runtime_message_id/message_id/client_message_id/request_id`，仅无稳定身份时才退回 index。
- 已补齐投影工具时间线：`ChatRuntimeMessageProjection` 支持 `workflowItems/subagents`，`tool_call_started/tool_call_completed/tool_call_failed` 会按 `tool_call_id/command_session_id/approval_id` 稳定 upsert 工具条目；render adapter 会把投影工作流复制为旧消息组件可读的 `workflowItems`，并在投影已产生工作流差异时禁止复用旧 raw 对象，避免投影模式下工具卡片丢失。
- 已补齐 subagent/team/approval 运行态投影：`subagent_*`、`team_*` 统一转为 `workflow_event`，reducer 按 `run_id/session_id/dispatch_id/task_id` 稳定更新同一条 workflow/subagent 记录；`approval_request/approval_result` 按 `approval_id` 合并同一审批卡片，并保持 `waiting_approval` 会话状态，避免审批等待被派生逻辑覆盖回普通 running。
- 已修正投影终态和 legacy 活动判断：terminal/idle 会同时收敛 workflowItems 与 subagents；旧快照即使只有 active subagents、没有 `workflowItems/workflowStreaming`，也会被视为 tooling/running，不再把仍在运行的子智能体误判成 idle。
- 已新增投影渲染刷新时钟：`runtimeProjectionVersion` 独立于旧 `messageMutationVersion`，所有 canonical/legacy projection 写入会按应用结果触发版本推进；流式 delta 默认用 `requestAnimationFrame` 或 16ms fallback 合并，提交、终态、snapshot、runtime status 等边界事件立即刷新。Messenger 在 projection/shadow 模式下读取 `toRaw(runtimeProjection)`，只依赖显式版本号重算，避免深层响应式追踪把 token 级事件扩散成整页重排。
- 已继续收敛显示态读取源：发送控制器的发送起点日志、待助手居中计数、停止确认快照改为读取 `resolveActiveAgentRenderableMessageRecords()`；`ChatComposer` 增加 `contextMessages` 输入，由 `MessengerView` 传入 `agentRenderableContextMessages`，输入区上下文占用和发送日志计数不再直接订阅旧消息数组。`test:messenger-renderable-source` 已锁定 message commands、composer、MessengerView 的读取来源。
- 已收紧 projection 渲染空列表语义：`chatRuntimeRenderAdapter` 暴露 `hasChatRuntimeRenderSession()`，Messenger 在 projection 模式下如果投影已知道当前 session，即使 renderable 列表为空也直接渲染空列表，不再自动回退 legacy `messages`；只有投影尚未见过该 session 时才允许 legacy fallback，避免空会话、清空面板或空快照被旧数组残留气泡污染。
- 已落地严格事件顺序缓冲：`chatRuntimeReducer` 为每个 session 增加 `pendingSequentialEvents` 有界缓冲，小范围 `event_seq` 乱序先等待缺失事件，不再让后续 delta/final 抢跑污染 projection；缺口补齐后按序排空并保持稳定 assistant message。超过缓冲能力的大 gap 会返回 `event_seq_gap` 并保留 `syncRequired`，当前兼容应用该事件，等后端连续 canonical seq 稳定后再升级为硬阻塞。
- 已接入 gap 主动恢复：`applyCanonicalStreamRuntimeEvent` 会把 `pending_event_seq_gap/event_seq_gap` 暴露给 watch/send/resume 入口；watch 小 gap 延迟一个 reconcile 窗口再拉取，硬 gap 立即 reconcile，send/resume 小 gap 延迟恢复、硬 gap 立即调用 `ensureActiveSessionRealtime`，避免只等 watchdog 才修复真实丢包。
- 已完成 legacy 流式输出可见刷新节流：`chatWorkflowProcessor` 不再在每个 `llm_output_delta` 上直接改气泡正文或 reasoning，而是按 40ms 基准窗口和下一帧合并刷新，最长等待受 `STREAM_FLUSH_MAX_MS` 约束；终态事件仍强制 flush。`notifySessionSnapshot` 同步增加可见签名闸门，无可见变化的流式 bookkeeping 不再推动 `messageMutationVersion`、legacy reconcile、窗口重算和快照落盘。新增 `test:chat-workflow-stream-flush` 并接入 `test:chat-realtime`。
- 当前剩余允许直接碰旧 `chatStore.messages` 的位置主要是会话删除/清空等写路径、legacy renderable builder、helper 内部 fallback；projection 渲染仍保持 feature flag，不直接默认切换。
- 下一步继续做 projection 默认切换前的场景压测：长流式乱序、真实工具调用、审批等待、子智能体运行、切会话与刷新恢复的 shadow/e2e 验证；当 render shadow 和 reducer shadow 在这些场景下稳定无 drift 后，再把 projection 模式从人工开关推进到小流量灰度默认，并继续削减旧 `messages` 数组直接写入路径。

### 阶段 0：冻结问题与观测

目标：不要盲改。

- 为现有链路增加 debug 开关下的事件轨迹：request id、event id、session id、stream round、消息 key、写入来源。
- 补充前端 reducer 单测，覆盖重复事件、乱序事件、gap、snapshot hydrate、terminal release。
- 补充后端 WS/stream event 集成测试，覆盖 queued 后继续执行、慢客户端、resume、cancel、error terminal。

交付物：

- 现有问题可复现脚本或测试夹具。
- 一份事件轨迹样例，能定位每条消息由哪个源写入。

### 阶段 1：后端 canonical event envelope

目标：先让后端事件具备稳定身份。

- 新增 canonical event 类型和 builder。
- 给 start/queue/final/terminal/tool 等边界事件补齐 turn id/message id/event seq。
- `GET /events` 返回 canonical fields。
- WS 继续兼容旧 payload，但额外携带 canonical fields。

交付物：

- Rust 单测覆盖 event seq 单调递增和必填字段。
- Postgres/SQLite 都能写入和读取新字段。

### 阶段 2：后端 turn state 快照

目标：恢复不依赖前端猜测。

- 新增 `chat_turn_state` 存储实现。
- 执行开始即创建快照。
- 边界事件更新快照。
- delta 只更新内存或节流写快照。
- terminal 删除或标记快照。
- 服务启动时标记遗留非 terminal 为 interrupted/recovering。

交付物：

- Rust 测试覆盖启动遗留快照、terminal 清理、错误中断、工具 timeline 恢复。

### 阶段 3：前端 canonical reducer/store

目标：让新 projection 能完整驱动聊天。

- 新增 canonical adapter，把现有 WS payload 转为 `ChatRuntimeEvent`。
- `chatTimelineStore` 持有 projection 和 cursor。
- subscription 统一处理 start ack、watch、resume、slow client backfill。
- 旧 `chatStore.messages` 暂时仍保留，但由 projection 单向同步用于兼容页面。

交付物：

- 前端单测覆盖 ack 绑定、assistant delta、final、cancel、snapshot reconcile、重复事件忽略。

### 阶段 4：Messenger 渲染切到 projection

目标：移除渲染层对旧可变数组的依赖。

- `agentRenderableMessages` 改为从 selector 读取。
- assistant live preview 由 `model_turn_id` 驱动。
- composer busy 状态由 projection runtime status 驱动。
- 虚拟列表 key 改为 stable message key。

交付物：

- 长流式输出时消息顺序不变。
- 切换会话再回来，partial runtime 或 interrupted 状态可恢复。

### 阶段 5：删除旧修补链路

目标：减少复杂度和隐性竞争。

可删除或降级的内容：

- `sendMessage` 中长期持有的 assistant placeholder 流处理。
- `startSessionWatcher` 里直接创建/复用 pending assistant 的逻辑。
- `resumeStream` 直接写 message 的逻辑。
- `cacheSessionMessages` 中的原地 assistant dedupe。
- 基于内容/时间/round 的 dedupe 和匹配。
- watchdog 触发 `loadSessionDetail` 后直接改消息数组的路径。

保留：

- HTTP snapshot，但只能 hydrate reducer。
- slow client 处理，但只能触发 backfill。
- demo mode 的最小兼容层，可单独隔离。

### 阶段 6：压测与收口

目标：证明重构有效。

- 1000/5000 条历史消息滚动不卡顿。
- 10 分钟长输出不假死。
- queued 后继续执行不丢事件。
- 断线重连后按 event id 追平。
- 快速切换 session 不重复 assistant。
- cancel 后 composer 立即释放，terminal 状态一致。

## 验收标准

- 同一 `model_turn_id` 最多只有一条最终 assistant 消息。
- `event_seq` gap 必须触发 backfill 或 snapshot，不允许静默跳过。
- queued ack 后后续事件不能依赖 watcher 兜底才能显示。
- 前端不能出现 send/watch/resume 同时写同一 message array 的路径。
- terminal event 到达、cancel 成功、socket disconnect 三类场景都能确定释放 composer。
- 渲染列表 key 在流式过程中不变化。
- 关闭网络再恢复后，最后一条 assistant 不丢、不重复、不乱序。
- 生产构建下长历史页面主线程不会因每 token 全量重算而卡住。

## 测试设计

### Rust

- `chat_events` 单测：event envelope 必填字段、seq 单调、idempotency key。
- `stream_events` 集成测试：persist-before-broadcast、resume after event id、gap replay。
- `chat_ws` 测试：queued 后 request 继续产生事件，subscribe 能收到。
- `chat_turn_state` 测试：started flush、delta 节流、terminal cleanup、startup interrupted。
- `cancel/error` 测试：所有异常路径都有 terminal event。
- SQLite 与 Postgres 都跑同一组存储契约测试。

### 前端

- reducer 单测：重复 event id、乱序 seq、gap、snapshot hydrate、terminal 结算。
- subscription 单测：断线重连、slow client backfill、session switch。
- send action 单测：client message ack 绑定、start ack 后不接管后续流。
- selector 单测：durable messages + live assistant preview 顺序稳定。
- dedupe 回归：同 content 的两轮 assistant 不被合并；同 id 重放不重复。

### E2E

- 长回复流式输出，滚动位置稳定。
- queued 请求执行完成，最终消息显示一次。
- 中途断开 WS，再恢复，消息补齐。
- 快速发送、取消、切换 session，composer 状态正确。
- 大历史加载、向上翻页、尾部流式输出同时发生时页面可交互。

## 文件落点建议

后端：

- `src/orchestrator/chat_events.rs`
- `src/orchestrator/chat_turn_state.rs`
- `src/api/chat_ws.rs`
- `src/api/chat.rs`
- `src/storage/sqlite.rs`
- `src/storage/postgres.rs`
- `tests/chat_realtime_recovery.rs`

前端：

- `frontend/src/realtime/chat/chatCanonicalEvents.ts`
- `frontend/src/realtime/chat/chatTimelineStore.ts`
- `frontend/src/realtime/chat/chatSubscription.ts`
- `frontend/src/realtime/chat/chatSnapshots.ts`
- `frontend/src/realtime/chat/chatTimelineSelectors.ts`
- `frontend/src/views/messenger/controller/*`

迁移期避免继续扩大 `frontend/src/views/MessengerView.vue` 和 `frontend/src/stores/chatRuntimeState.ts`。超过维护阈值的大文件只做接线和删除旧路径，新逻辑放到新模块。

## 风险与处理

- 风险：新旧协议并行期间出现双写。处理：新 projection 开 feature flag，只允许一个渲染源生效。
- 风险：persist-before-broadcast 增加延迟。处理：只对 boundary events 强同步；delta 聚合后批量持久化和广播。
- 风险：旧历史没有 turn id/message id。处理：snapshot hydrate 时生成 `legacy:<history_id>` 稳定 key，但新消息必须使用服务端 id。
- 风险：桌面 SQLite 写入频繁。处理：delta batch、turn state 节流、terminal 一次性落 durable message。
- 风险：迁移周期中 demo mode 或离线路径破坏主链路。处理：demo adapter 输出同一 canonical event，不再单独改 messages。

## 最小先行改动

如果需要在完整重构前先止血，优先做三件事：

1. `sendMessage` 的 WS request 在 queued ack 后保持 pending，或改为 queued 后立即启动 subscribe，不让后续 request-scoped 事件静默丢弃。
2. 禁止 watch/resume 在发送流活跃时直接创建新的 assistant message，只能补同一个 `model_turn_id` 或触发 snapshot。
3. `cacheSessionMessages` 不再默认原地 dedupe，改为仅在明确的 legacy hydrate 阶段执行，并打 debug 日志。

这些只是止血，不替代 canonical event + single reducer 的重构。
