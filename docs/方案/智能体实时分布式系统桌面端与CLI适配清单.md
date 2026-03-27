# 智能体实时分布式系统桌面端与 CLI 适配清单

## 1. 结论与边界

- 这套方案对 `server/web` 的影响最大，对 `desktop` 的影响中等，对 `cli` 的影响小到中等。
- 桌面端和 CLI 不应该被改造成“本地集群版 server”，而应该共享同一套智能体实时内核，并在部署层折叠为单机嵌入式模式。
- 本地形态的目标不是复制分布式部署拓扑，而是复用分布式语义：统一的 thread runtime、mission runtime、projection event、snapshot/replay、ack/backpressure、优先级队列。
- 本地形态必须继续以智能体为中心，projection/UI/TUI 只能消费智能体运行事件，不能反向污染智能体认知状态。
- 线程 `system prompt` 首次确定后继续冻结；长期记忆继续只允许在线程初始化时注入一次。

## 2. 当前代码判断

- [src/core/state.rs](C:/Users/sjxx/Desktop/wunder/src/core/state.rs) 中 `cli_default()` 默认关闭 `start_mission_runtime`、`start_thread_runtime`、`start_cron`。
- [src/core/state.rs](C:/Users/sjxx/Desktop/wunder/src/core/state.rs) 中 `desktop_default()` 默认关闭 `start_mission_runtime`，启用 `start_thread_runtime` 与 `start_cron`。
- [wunder-cli/runtime.rs](C:/Users/sjxx/Desktop/wunder/wunder-cli/runtime.rs) 的 `apply_cli_defaults()` 已将 `server.mode` 设为 `cli`，并关闭 `channels`、`gateway`、`agent_queue`、`cron`，说明 CLI 当前是轻量本地形态，不是完整 server 形态。
- [desktop/tauri/runtime.rs](C:/Users/sjxx/Desktop/wunder/desktop/tauri/runtime.rs) 的 `apply_desktop_defaults()` 已将 `server.mode` 设为 `desktop`，并使用本地 SQLite、工作区根目录和桌面侧默认配置。
- [src/storage/sqlite.rs](C:/Users/sjxx/Desktop/wunder/src/storage/sqlite.rs) 已对本地 SQLite 启用 `busy_timeout`、`journal_mode=WAL`、`synchronous=NORMAL`，本地高频事件写入已有较好基础。
- [src/api/mod.rs](C:/Users/sjxx/Desktop/wunder/src/api/mod.rs) 的 `build_desktop_router()` 已挂载 `chat`、`chat_ws`、`core_ws`、`beeroom_ws`、`user_world_ws` 等实时入口，桌面端已经具备承接统一 realtime 协议的入口面。
- [desktop/tauri/bridge.rs](C:/Users/sjxx/Desktop/wunder/desktop/tauri/bridge.rs) 已通过本地 Axum bridge 承载桌面端 API，并用 desktop token 做本地访问保护。
- [src/api/desktop_lan.rs](C:/Users/sjxx/Desktop/wunder/src/api/desktop_lan.rs) 已存在桌面 LAN `peers / envelope / ws` 能力，这适合作为可选外层投影网络，而不是核心智能体 owner 路径。
- [wunder-cli/main.rs](C:/Users/sjxx/Desktop/wunder/wunder-cli/main.rs) 已同时支持 `orchestrator.run()` 和 `orchestrator.stream()`。
- [wunder-cli/tui/app.rs](C:/Users/sjxx/Desktop/wunder/wunder-cli/tui/app.rs) 已具备 `drain_stream_events()` 和流式渲染节流基础，CLI 已经接近“终端投影客户端”，适合接入统一事件语义。

## 3. 适配原则

- 统一智能体实时内核，分离部署形态适配层。
- `server_distributed`、`desktop_embedded`、`cli_embedded` 三种模式共享同一套 runtime 接口，不共享同一套部署负担。
- 本地形态折叠规则必须明确：
- `owner routing` 折叠为进程内 owner。
- `lease` 折叠为本地 epoch 或 no-op lease。
- `pubsub` 折叠为进程内 event bus。
- `presence / directory` 折叠为本地缓存，可选叠加 LAN overlay。
- `snapshot / replay` 统一走本地 SQLite 持久化和 checkpoint。
- 高优事件永不丢弃：用户消息入队、模型开始执行、工具开始/结束、最终答案、错误、取消、智能体关键状态切换。
- 低优事件允许合并：presence、心跳、调试指标、重复工具进度、次级统计。
- 慢客户端默认依赖 `snapshot + delta + replay + backpressure + priority lanes`，而不是大范围全量刷新。
- desktop/cli 只能新增“嵌入式 runtime 模式”，不能强迫本地端依赖 Postgres、Redis、Kafka 或独立 agent queue 进程。

## 4. 共享内核适配清单

### 4.1 运行模式收口

- [ ] 在 [src/core/state.rs](C:/Users/sjxx/Desktop/wunder/src/core/state.rs) 引入显式 deployment/runtime profile，至少区分 `server_distributed`、`desktop_embedded`、`cli_embedded`。
- [ ] 将当前 `start_mission_runtime`、`start_thread_runtime` 这样的布尔开关继续收口为更稳定的 profile 级别启动策略，避免未来组合爆炸。
- [ ] 为 desktop/cli 增加嵌入式 thread runtime 启动能力，而不是沿用 server 的全局扫描式后台组件。
- [ ] 为 shared runtime 增加 capability 描述，明确当前节点是否支持 mission、projection、LAN overlay、background cron、gateway 等能力。

### 4.2 本地 owner / bus / replay 折叠层

- [ ] 在 `src/services/runtime/local/` 新增单机适配层，承接嵌入式 owner、进程内 pubsub、local presence、event replay。
- [ ] 复用 [src/services/stream_events.rs](C:/Users/sjxx/Desktop/wunder/src/services/stream_events.rs) 作为 replay/checkpoint 的底座，不再让各入口直接各自拼装 replay 逻辑。
- [ ] 将 websocket 重连后的 `resume` 能力从“轮询存储兜底”升级为“优先进程内 tail bus，缺口再回放存储”。
- [ ] 统一事件 envelope，保证 desktop、cli、web 收到的 thread/mission/projection 事件语义一致。
- [ ] 对每条事件补齐 `event_id / lane / priority / target / runtime_epoch / projection_version` 元数据。

### 4.3 事件优先级与背压

- [ ] 定义关键优先级通道：`critical`、`interactive`、`projection`、`diagnostic`。
- [ ] 关键通道采用严格有序、不丢事件策略。
- [ ] 非关键通道允许 coalesce，例如智能体心跳、工具耗时 tick、presence 变化。
- [ ] 单连接、单 TUI、单窗口都要有 bounded queue，避免慢消费者反向拖垮运行内核。
- [ ] 对投影消费者补齐 `resync_required`、`snapshot_required`、`replay_required` 三类恢复信号。

## 5. 桌面端适配清单

### 5.1 启动与进程模型

- [ ] 在 [desktop/tauri/runtime.rs](C:/Users/sjxx/Desktop/wunder/desktop/tauri/runtime.rs) 增加 `desktop_embedded` runtime profile 初始化。
- [ ] 桌面端默认启动嵌入式 thread runtime；mission runtime 按需懒启动，不走 server 侧全局 runner 常驻策略。
- [ ] 保持 [desktop/tauri/bridge.rs](C:/Users/sjxx/Desktop/wunder/desktop/tauri/bridge.rs) 为桌面端唯一对外 bridge，不新增第二套本地 server。
- [ ] 桌面端 capability bootstrap 中增加 `embedded_kernel`、`projection_lanes`、`replay_window`、`slow_client_policy` 等字段，供前端决定订阅策略。

### 5.2 本地实时链路

- [ ] 统一 [src/api/core_ws.rs](C:/Users/sjxx/Desktop/wunder/src/api/core_ws.rs)、[src/api/chat_ws.rs](C:/Users/sjxx/Desktop/wunder/src/api/chat_ws.rs)、[src/api/beeroom_ws.rs](C:/Users/sjxx/Desktop/wunder/src/api/beeroom_ws.rs)、[src/api/user_world_ws.rs](C:/Users/sjxx/Desktop/wunder/src/api/user_world_ws.rs) 的 handshake、ack、resume、priority、snapshot 语义。
- [ ] 桌面端消息发送后必须先本地 echo，再等待 runtime `accepted/queued` 事件确认。
- [ ] 用户直接发送、渠道回流、蜂群调用三种来源都必须先进入同一 event bus，再分发到 thread 和 projection，不允许各写各的前端状态。
- [ ] 智能体状态、工具状态、最终答案必须共享统一序列号体系，避免前端出现“消息到了但状态还没到”或“工具结束了但最终答案晚一拍”的分裂。
- [ ] reconnect 优先走 snapshot + replay，不依赖页面全量 reload 修正状态。

### 5.3 前端投影与慢客户端

- [ ] 桌面端前端沿用用户侧统一投影 runtime，不做桌面特供状态机分叉。
- [ ] 关键可见区只订阅当前 thread/mission/active projection，后台页自动降级低优通道。
- [ ] 慢客户端或弱机型下优先保留用户消息、最终答案、工具状态切换，合并中间 token 和频繁进度事件。
- [ ] 列表页、会话页、蜂群页全部改为 snapshot + delta 驱动，禁止周期性整包 refresh 作为 steady-state 正常路径。
- [ ] 页面恢复时先恢复最近 snapshot，再补 replay，不等待所有低优通道完全同步后才渲染首屏。

### 5.4 LAN overlay 边界

- [ ] [src/api/desktop_lan.rs](C:/Users/sjxx/Desktop/wunder/src/api/desktop_lan.rs) 保持为可选 overlay，不参与核心 thread owner 判定。
- [ ] LAN 来的 envelope 统一映射为外部输入事件，再进入本地 embedded kernel，不允许绕过主 event bus 直写 projection。
- [ ] LAN peer 视图只影响投影目录和可见性，不得直接修改本地智能体线程 cognition。
- [ ] LAN 同步失败时仅影响跨设备感知，不影响本机核心会话执行。

### 5.5 本地存储与恢复

- [ ] 继续使用 SQLite + WAL 作为桌面端 event log / snapshot / checkpoint 主存储。
- [ ] 为桌面端补充 projection snapshot 表或 checkpoint 表，避免每次启动都从事件头重建可见状态。
- [ ] 将高频 tool/progress/status 事件写入批量化，避免单事件单事务放大。
- [ ] 为“最近活跃 thread / mission / beeroom / user_world”建立启动时快速恢复路径。
- [ ] 加入本地数据库损坏、WAL 残留、锁冲突时的只读恢复与重建策略。

## 6. CLI 适配清单

### 6.1 启动模式

- [ ] 在 [wunder-cli/runtime.rs](C:/Users/sjxx/Desktop/wunder/wunder-cli/runtime.rs) 增加 `cli_embedded` runtime profile。
- [ ] 一次性命令保留轻量路径，但交互式 chat/TUI 默认启用嵌入式 thread runtime。
- [ ] CLI 不启完整 gateway/channels/cron，但要能消费统一 runtime 事件语义。
- [ ] CLI 的 embedded runtime 默认 headless，不暴露本地监听端口。

### 6.2 事件消费与渲染

- [ ] 将 [wunder-cli/main.rs](C:/Users/sjxx/Desktop/wunder/wunder-cli/main.rs) 的 JSONL/stdout renderer 和 [wunder-cli/tui/app.rs](C:/Users/sjxx/Desktop/wunder/wunder-cli/tui/app.rs) 的 TUI sink 统一到同一事件适配层。
- [ ] TUI 继续保留 `drain_stream_events()` 的预算控制，但预算要按优先级通道拆分，防止 tool spam 挤压最终答案。
- [ ] `--no-stream` 不能再走独立字段拼装语义，而应从统一 final projection / final event 中取结果。
- [ ] CLI 恢复会话、统计会话、查看后台任务，都应优先读取 shared replay/checkpoint API，不再直接散落读取底层表。
- [ ] 终端宽度不足、渲染跟不上时，降级为摘要模式，只保留关键状态切换和最终输出。

### 6.3 会话与恢复

- [ ] CLI 会话切换要绑定 runtime epoch，避免旧会话残余事件串到新会话。
- [ ] 会话恢复默认走 `snapshot -> replay -> live tail` 三段式，而不是“直接继续连流”。
- [ ] JSONL 模式必须保证事件有序、可重放、可增量解析，便于外部自动化接入。
- [ ] 本地中断、Ctrl+C、终端关闭后，下一次恢复要能明确识别：已完成、已取消、仍在后台、需重新附着。

### 6.4 本地数据库与清理

- [ ] CLI 继续使用本地 SQLite，但分清 session 元数据、event log、projection checkpoint、临时缓存的生命周期。
- [ ] 为 CLI 增加 event log 截断/压缩策略，避免长期交互导致本地库无限增长。
- [ ] 为批处理命令和交互式命令增加不同的 retention policy。
- [ ] 在异常退出后增加 orphaned replay cursor 清理与修复流程。

## 7. 配置、观测与兼容清单

- [ ] 在配置中增加 `runtime_profile`、`embedded_runtime.enabled`、`projection.replay_window`、`projection.max_queue`、`projection.slow_client_policy`。
- [ ] 为 desktop/cli 增加单独监控项：本地队列深度、事件丢弃数、coalesce 次数、snapshot 命中率、重放耗时、UI/TUI 渲染延迟。
- [ ] 不再保留 legacy/hybrid 切换开关，desktop/cli 直接对齐 embedded runtime 主路径。
- [ ] server、desktop、cli 统一围绕 thread runtime / mission runtime / projection runtime 组织，不再保留旧 `AgentRuntime/TeamRunRunner` 外壳。
- [ ] 任何运行时重构都不能改写线程冻结的 `system prompt` 和一次性注入的长期记忆。

## 8. 分阶段节点

### R0 现状收口

- [ ] 确认 desktop/cli 各入口的 runtime profile 和 capability 输出。
- [ ] 列出所有当前直接读写 stream events、直接修改 projection 状态的路径。
- [ ] 给 desktop/cli 增加 profile 级诊断输出，便于排查本地模式差异。

### R1 共享嵌入式内核

- [ ] 引入本地 owner、进程内 bus、shared replay 接口。
- [ ] desktop/cli 都能以 embedded 方式启动 thread runtime。
- [ ] server/desktop/cli 统一切到新的 thread runtime 主语义。

### R2 桌面端接入

- [ ] 桌面 bridge 输出 shared capability。
- [ ] chat / beeroom / user_world / core ws 统一 resume 语义。
- [ ] 前端首屏改为 snapshot + replay + live tail。

### R3 CLI 接入

- [ ] CLI one-shot、interactive、TUI 三条路径统一事件模型。
- [ ] `--json`、`--no-stream`、TUI 使用同一最终事件定义。
- [ ] 会话恢复改为 shared replay API。

### R4 慢客户端与背压

- [ ] 完成关键/低优通道拆分。
- [ ] 完成 queue 上限、coalesce、resync_required。
- [ ] 慢客户端不再拖慢 embedded kernel。

### R5 存储与恢复

- [ ] 桌面端 recent projection 快速恢复上线。
- [ ] CLI event log retention 与 compaction 上线。
- [ ] 本地异常恢复路径完成。

### R6 默认启用与回归

- [ ] 桌面端默认开启 embedded runtime profile。
- [ ] CLI 交互式默认开启 embedded runtime profile。
- [ ] 清理旧 runtime 名称、旧切换开关与回滚分支。

## 9. 验收指标

- [ ] 同机桌面端用户发消息到前端本地 echo，目标 P95 小于 `16ms`。
- [ ] 同机桌面端从发送到收到 runtime `accepted/queued` 关键确认事件，目标 P95 小于 `80ms`。
- [ ] 同机桌面端从工具状态变化到前端可见，目标 P95 小于 `120ms`。
- [ ] CLI TUI 在高频流式输出时，键盘输入到界面响应延迟目标 P95 小于 `100ms`。
- [ ] CLI JSONL 模式在断线恢复后不丢关键事件、不重复最终答案。
- [ ] 桌面端或 CLI 在慢消费者场景下，embedded kernel 的主执行延迟不因单个前端/TUI 堵塞而线性恶化。
- [ ] 用户直接发送、渠道回流、蜂群调用三种入口都能在统一事件序列中观察到一致的消息与状态顺序。
- [ ] 任意恢复路径都不能出现“消息正文已到、智能体状态未到”或“工具结束已到、final 迟到且乱序”的可见撕裂。

## 10. 非目标与禁止事项

- [ ] 不在桌面端和 CLI 上强依赖完整分布式基础设施。
- [ ] 不为桌面端和 CLI 复制一套独立智能体行为逻辑。
- [ ] 不让 LAN overlay 参与核心智能体 owner 决策。
- [ ] 不让 projection 层直写线程认知状态。
- [ ] 不通过大范围全量刷新掩盖状态同步设计问题。
