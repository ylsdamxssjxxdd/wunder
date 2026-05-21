# openhuman 聊天设计分析

## 结论摘要

`C:\Users\sjxx\Desktop\参考项目\openhuman-main` 的聊天稳定性来自清晰的状态分层：持久消息、实时运行态、socket 事件、冷启动恢复各自有单一职责。前端不把 token 流直接当作最终消息反复改写，而是用 `chatRuntimeSlice` 保存轻量的流式预览和工具时间线，用 `threadSlice` 保存已经落库的线程消息。后端用 `ConversationStore` 持久化线程与消息，用 `TurnStateStore` 记录正在执行的 turn 快照，完成后删除快照，异常或重启时标记为 `Interrupted`。

这个设计不依赖内容相似度、时间戳猜测或多条恢复链路互相修补。它的核心经验是：聊天页面只消费一个确定性的运行态投影，最终消息只由明确的终态事件写入持久消息层。

## 主要文件

- 前端页面：`app/src/pages/Conversations.tsx`
- 全局运行态订阅器：`app/src/providers/ChatRuntimeProvider.tsx`
- 聊天 socket/RPC 封装：`app/src/services/chatService.ts`
- 持久线程与消息状态：`app/src/store/threadSlice.ts`
- 实时运行态状态：`app/src/store/chatRuntimeSlice.ts`
- 线程与 turn state RPC：`app/src/services/api/threadApi.ts`
- 后端线程 RPC：`src/openhuman/threads/*`
- 后端 turn 快照：`src/openhuman/threads/turn_state/*`
- 后端消息存储：`src/openhuman/memory/conversations/*`
- 后端 agent turn 主链路：`src/openhuman/agent/harness/session/turn.rs`

## 用户聊天闭环

`Conversations.tsx` 发送消息时先生成一个本地 user message，通过 `addMessageLocal` 写入 `threadSlice`，然后标记 `beginInferenceTurn`、`setActiveThread`，最后调用 `chatSend`。`chatSend` 不直接接管流式结果，它只通过 RPC 把请求提交给 Rust core，并带上 socket client id，让后端把事件路由回当前 socket。

消息发送后的 UI 更新主要由 `ChatRuntimeProvider.tsx` 统一处理。页面自身不同时订阅多个流，也不在发送函数里长期维护 assistant 占位消息。页面渲染时从两类状态读取：

- `threadSlice.messagesByThreadId`：已经持久化的 user/agent 消息。
- `chatRuntimeSlice`：当前 turn 的状态、工具 timeline、task board、流式 assistant preview。

这种拆分使发送函数保持短链路，实时事件处理集中，页面渲染不会和网络请求生命周期强耦合。

## 前端状态分层

`threadSlice.ts` 负责 durable thread model。线程列表、线程消息、追加消息、更新 reaction、删除线程等都在这里。agent 最终回复会通过 `addInferenceResponse` 追加到持久消息层。

`chatRuntimeSlice.ts` 负责 in-flight turn model。它维护：

- `inferenceStatusByThread`：思考、工具调用、子智能体等阶段。
- `streamingAssistantByThread`：流式文本和 thinking 文本。
- `toolTimelineByThread`：工具与子智能体时间线。
- `taskBoardByThread`：任务看板。
- `inferenceTurnLifecycleByThread`：`started`、`streaming`、`interrupted`。

运行态不会长期冒充持久消息。比如纯文本 token 流只更新 `streamingAssistantByThread`，页面用最多 120 个尾部字符做预览，最终完整内容在 `chat_done` 时才进入 durable messages。这降低了每个 token 触发整条消息列表重排的风险。

## 实时事件协调器

`ChatRuntimeProvider.tsx` 是前端唯一的聊天事件协调器。它调用 `subscribeChatEvents` 一次性订阅标准事件名，包括 `inference_start`、`iteration_start`、`tool_call`、`tool_result`、`text_delta`、`thinking_delta`、`chat_segment`、`chat_done`、`chat_error`、`task_board_updated` 和子智能体事件。

`chatService.ts` 只订阅 snake_case 规范事件名。代码里明确说明后端可能发兼容别名，但前端只订阅一套规范事件，避免同一个逻辑事件被处理两遍。

事件处理具备三类防抖和幂等机制：

- 事件级去重：`seenChatEventsRef` 用 thread id、request id、事件类别和关键字段组成 key。
- 分段回复去重：`segmentDeliveriesRef` 按 thread/request 记录已经收到的 segment index。
- 子智能体工具调用去重：同一个 `tool_call_id` 不重复追加。

需要注意的是，openhuman 的去重仍有少量内容 digest 用于 proactive message，但主聊天路径不是靠内容相似度猜测消息归属。

## 终态写入策略

`chat_done` 是最终 assistant 消息进入持久层的关键点：

- 如果没有 segment，则用 `full_response` 追加一条 agent message。
- 如果有 segment 且 segment 已完整到达，则 segment 过程已经追加，不再重复追加 full response。
- 如果 segment 不完整但 `full_response` 存在，则走 reconcile 路径追加完整回复。

一个关键细节是：分段是否完整只看 `segment_index` 是否覆盖 `0..segment_total-1`，不再拿拼接后的分段文本和 `full_response` 做字节级比较。项目注释说明服务端分段会裁剪和规范化展示文本，而 `full_response` 保留原始 LLM 文本，字节比较会误判并产生重复 assistant 消息。

## 后端消息存储

`ConversationStore` 使用 JSONL 存储线程和消息：

- `threads.jsonl` 是线程元数据的 append-only upsert/delete 日志。
- 每个线程一个消息 JSONL 文件，位于 `memory/conversations/threads/<id>.jsonl`。
- 写入通过进程级 mutex 串行化，避免并发 RPC 交错写文件。
- 线程列表通过 `MessageAppended` 统计记录避免每次扫描全部消息。

这套存储不是高并发 server 的最终形态，但结构上有参考价值：线程消息是持久事实，运行态是旁路状态，不把流式 token 与最终消息混在一个可变数组中。

## TurnState 冷启动恢复

`src/openhuman/threads/turn_state` 是 openhuman 最值得借鉴的部分。`TurnState` 的结构和前端 `chatRuntimeSlice` 对齐，包含 thread id、request id、生命周期、iteration、phase、active tool、active subagent、streaming text、thinking、tool timeline、task board、时间戳。

`TurnStateStore` 用每线程一个 JSON 文件保存快照，写入采用临时文件加 rename 的方式原子覆盖。启动时可以 `mark_all_interrupted`，把遗留的非终态快照标为 `Interrupted`，UI 据此展示重试入口，而不是伪造一个仍在运行的流。

`TurnStateMirror` 监听 agent progress：

- turn 开始时立即 flush，避免崩溃后没有恢复记录。
- iteration、工具开始、工具完成、task board 更新等边界事件 flush。
- 文本 delta、thinking delta、工具参数 delta 只更新内存，不每个 token 刷盘。
- turn completed 删除快照。
- bridge 退出但未观察到 completed 时，将快照标记为 interrupted 后持久化。

这个策略兼顾恢复能力和性能，避免高频 token 写盘拖慢页面和后端。

## 渲染与性能策略

openhuman 的聊天页没有把所有实时 token 都写进持久消息数组。流式文本在运行态中作为 preview 读取，页面只展示尾部片段。这让长回复流式输出时不会频繁改写大数组，也减少滚动跳动。

工具 timeline、task board、streaming assistant 都按 thread id 独立保存。用户切换线程后，运行态仍可恢复显示；最终消息仍从 thread messages 读取。页面对当前线程做派生选择，而不是让每条 socket 事件直接操作当前可见 DOM 列表。

## 对 wunder 的可借鉴点

1. durable messages 和 live turn runtime 必须拆开。最终消息只由明确的终态或服务端消息事件进入持久消息层。
2. 前端需要一个全局、单一的事件协调器。发送函数只提交请求，不长期持有消息流控制权。
3. 后端需要 turn state 快照。崩溃、刷新、断线后用快照恢复运行态或标记 interrupted，不让前端靠时间戳和内容猜测。
4. 事件协议必须提供稳定 id：thread/session id、request id、user turn id、model turn id、message id、event seq/event id。
5. 流式 token 应写入轻量 runtime buffer，并节流渲染；最终消息再进入 durable history。
6. 分段/恢复判断应基于序号和状态，不基于文本相似度或字节比较。
7. 断线时要明确释放输入锁，同时保留 partial preview；不要让 composer 依赖一个永远等不到的 terminal event。

## 不宜直接照搬的部分

openhuman 的本地 JSONL 和单进程 mutex 适合 desktop/local core，不适合 wunder server 的多租户并发形态。wunder server 应继续以 Postgres 为主，desktop 可用 SQLite，但可以借鉴它的边界设计：消息表是事实表，turn state 是可覆盖快照，stream events 是可重放日志。

另外，openhuman 的前端是 React/Redux，wunder 用户侧是 Vue3/Pinia。迁移时不需要照搬 Redux slice 形式，应该在 `frontend/src/realtime/chat/` 和 `frontend/src/stores/` 中建立等价的规范 reducer 和 projection。
