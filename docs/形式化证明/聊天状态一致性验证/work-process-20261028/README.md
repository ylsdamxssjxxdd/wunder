# 工作过程实时性与条目身份验证

日期：2026-10-28。对象：蛹本地 AI 工作台的原生聊天投影、蛹云端线程，以及蜂巢云端 AI 工作台的工作过程。修复共享运行时与持久投影，再验证客户端；不是整个运行时已被形式化证明。

## 根因和契约

1. 连续结构事件反复重置尾沿 debounce，600 ms 窗口不断后移。现在保留首次 120 ms 期限，在途读取保留一次 dirty 需求；文本增量按帧合并，不逐 token 请求过程页。
2. 不同模型轮次被按文本相等或前缀合并，导致新思考进入旧完成条目。现在按 canonical `execution:text-model_round` 区分；终态不退回运行态，折叠选择保留。新轮次首帧有游标缺口时不先清空旧条目。
3. 拒绝旧身份后仍追加增量，以及新 revision 携带旧模型快照。现在身份、模型轮次、UTF-16 offset 分别校验；持久更新保留更晚的活动流，不能把旧正文与新 reasoning ID 配对。
4. compact 订阅只推已完成快照，工具开始和旁观/云端思考要等完成才可见。运行时增加有界 `thread_process_item`，共用 live hub 推送 `thread_item_tail`；直接执行流不再重复订阅自己的实时帧。读取数据库仅在持久/控制状态唤醒时发生，不随每个 token 查询。
5. 并发工具完成顺序不是开始顺序。运行时在 emit 串行闸内分配 `process_seq`，随同 immutable 结果持久化；两端使用相同的稳定排序规则。云端在线 overlay 修复逆序持久结果，并且不污染用户正在浏览的较旧页面。
6. 标题原计数包含 model_call、内部记录和当前回答，不能代表可见过程条目。增加独立 `process_count`，保留旧 workflow_count 含义；计数包括思考、中间正文、工具和压缩记录，排除当前最终回答与内部项。
7. 长过程验证额外发现 canonical 包装的 `configuration` / `tool_result` 遮住语义 kind，导致恢复计数和尾窗外子智能体卡丢失。共享 compact summary 按 payload 的语义类型投影，保留最多 8 张子智能体卡；不靠全量扫描历史恢复。

## 模型与结论

权威源在 `../models/ProcessIdentity.*`、`../models/ProcessDelivery.*`、`../models/ThreadHead.*`。runner 复制到仓库外；前两组生成只把 `Fixed = FALSE` 改为 `TRUE` 的对照配置，ThreadHead 使用独立固定模型和固定配置；均不删除原不变量。

| 模型 | 修复前穷举 | 反例 | 修复后 |
| --- | --- | --- | --- |
| ProcessIdentity | 9 状态、6 转移 | 已完成思考被前缀合并覆盖；旧增量回滚；旧快照回滚，共 3 类 | 同组 4 条不变量通过 |
| ProcessDelivery | 12 状态、9 转移 | 首次刷新期限后移；运行工具不可见；逆序完成改变顺序，共 3 类 | 同组 6 条不变量通过 |
| ThreadHead | 9 状态、9 转移 | 传输收尾误报完成、终态气泡未同步、迟到运行快照重开，共 5 类 | `TypeOK` 与 4 条业务不变量通过 |

`ProcessDelivery` 保留 `SettledToolAgreement`、`NoFabricatedSuccess`；此前 `ProcessRefreshFixed` 的三条不变量继续保留，独立基线复验为 39 状态、无反例。这里不是用新模型替换旧证明。

归档分别见 `identity/`、`delivery/` 的 summary、stories、总图与编号反例图；ThreadHead 的独立证据见 [终态与传输收尾续验证](../thread-head-20261028/README.md)。蓝色边框表示编号路径；绿色框表示“被修复或因修复新增”的对照差异，不表示框内旧错误状态正确。判断以同名规则、stories 的修复后目标和 `chartStatus.wrong = 0` 为准。图已人工查看。

| 编号与规则 | 编号步骤 | 复现 / 修复 | 图与 Mermaid |
| --- | --- | --- | --- |
| identity-1 `CompletedReasoningImmutable` | 1 下一轮开始 → 2 按条目匹配时覆盖旧思考 | 旧模型复现，同前缀真实回归；修复后不覆盖 | [PNG](./identity/bug-1.png) / [源码](./identity/bug-1.mmd) |
| identity-2 `NoDurableIdentityRollback` | 1 下一轮开始 → 2 接收旧模型快照 | 旧模型复现，真实 store/reducer 回归；修复后身份不退 | [PNG](./identity/bug-2.png) / [源码](./identity/bug-2.mmd) |
| identity-3 `NoTailIdentityRollback` | 1 下一轮开始 → 2 接收旧轮次增量 | 旧模型复现，真实 store/reducer 回归；修复后拒绝追加 | [PNG](./identity/bug-3.png) / [源码](./identity/bug-3.mmd) |
| delivery-1 `LiveToolVisible` | 1 工具开始即推送条目 | 旧模型不可见，真实 emitter 与 DOM 回归；修复后执行中可见 | [PNG](./delivery/bug-1.png) / [源码](./delivery/bug-1.mmd) |
| delivery-2 `FirstDeadlineNotPostponed` | 1 首次结构事件 → 2 后续事件推迟期限 | 旧模型复现，连续事件真实 DOM 回归；修复后期限不变 | [PNG](./delivery/bug-2.png) / [源码](./delivery/bug-2.mmd) |
| delivery-3 `ToolOrderStable` | 1 启动工具一、二 → 2 工具二先完成 | 旧模型顺序 2、1，真实分页/overlay 回归；修复后 1、2 | [PNG](./delivery/bug-3.png) / [源码](./delivery/bug-3.mmd) |

完整路径文字见 [identity stories](./identity/stories.md)、[delivery stories](./delivery/stories.md)；全图分别为 [identity PNG](./identity/chart.png) / [Mermaid](./identity/chart.mmd)、[delivery PNG](./delivery/chart.png) / [Mermaid](./delivery/chart.mmd)。

模型只抽象两个模型轮次、两个工具、最多四次结构输入的安全性与有限收敛。`count < 4`、phase 和 scenario 是 fixture 边界，不是生产守卫；逻辑时钟的 3 不是 120 ms 的证明。未建模的网络交付、公平性或故障不能从这些模型推出。

## 代码映射

| 状态/动作 | 实现与真实回归 |
| --- | --- |
| oldText / incomingText / BindReasoning | `frontend/src/components/chat/MessageTimelineTurnProcess.vue` 的 canonical reasoning key；`frontend-slint/src/timeline_text.rs::merge_reasoning_tail`；同前缀/相同正文回归 |
| liveRound / visibleRound / currentText / NextRound / LateTail / DurableRefresh | `frontend/src/realtime/chat/threadViewStore.ts` 的 bindTailAssistant、applyThreadViewTail、行合并；`frontend-slint/src/cloud_threads.rs` 的 apply_tail_to、merge_messages；本地 TurnOutput 身份守卫 |
| deadline / firstDeadline / StructuralEvent / DeadlineFires | Vue 不重置 timer + processRefreshGate；真实浏览器连续事件回归 |
| visibleTool / toolStatus / processItemsStatus / ToolStarts / ToolFinishes / LateToolStart | runtime event_stream + thread_change_hub、WS display watch；两端 live/durable upsert 与 terminal 守卫 |
| order / ConcurrentToolStarts / ReversedToolCompletion | runtime assign_process_order；native sort_process_items / cloud overlay；Vue rebuildRows；逆序完成回归 |
| scenario / phase / count / clock / requestStarted / status | 有限测试输入与同步步骤抽象；源码行注释提供可打开的检查点，行号为本次快照而非永久标识 |

## 实测证据

- 浏览器真实 Vue DOM：11/11 通过。受控 API fixture 的持续事件场景首次刷新约 **124.4 ms**，运行工具在线可见约 **4.0 ms**，旧完成思考保持不变，活动思考在末尾；不能当外部模型或部署 SLA。
- runtime：compact summary 6、canonical SQLite 12、change stream 20（其中本次专门 work_process 3）、hub 6 项定向回归；真实 emitter 证明 running tool 在结果之前可见，1100 个待完成工具不淘汰首工具开始序号。
- native façade：4/4，包括 1605 个工具跨多页完整遍历、最终回答尾窗外恢复、过程计数 1606、子智能体卡恢复及逆序结果排序；每页最多 100 项，初始聊天尾窗最多 24 项。
- Slint：云端 9、原生 reducer 21、reasoning 合并 4、过程分页 5 项定向测试；真实 ToolProcess 的 slint-viewer 渲染已检查。完整 main 的 viewer 检查被其他设置页 `[string].index-of` 诊断阻挡，未修改无关设置页，也不声称完整界面视觉验收通过。
- 独立 PostgreSQL 16：service_chat 纯投影 3、真实执行头 1、受理/生命周期 4 项；包含清空 fixture v2 派生索引后受理新输入，验证旧输入、状态和计数恢复。开启 `postgres-storage`；不是 `running 0 tests`。
- Vue 三组定向回归与类型检查通过；Python 证明 runner 4、资料布局 6、外部体验脚本 27 项通过。runner 额外拒绝缺少任一不变量日志、探索异常和仓库内报告路径。
- WebSocket 11/11：覆盖结构帧等待容量、文本尾帧有界丢弃、live hub overflow 后发送补水信号并继续同一订阅、控制状态无游标推进时唤醒，以及 queue-owned 最终回执。慢客户端的 durable/control 帧使用可取消背压，不静默结束订阅；这些测试不将排队等待时间包装成毫秒级保证。

## 资源与迁移边界

- live hub 每线程 128 帧；无订阅不保留事件。丢失 text tail 以 offset 阻止拼接伪连续正文，后续持久快照修复；结构帧至多等待 2 秒，溢出触发明确补水，读侧等待可取消，不能无限累积队列。
- Web 同一线程跨所有用户轮次的在线条目合计最多 100；蛹云端在线条目最多 100，过程子行最多 201。历史页有后续页时不混入在线尾部。原生并发读最多 4，同过程在途需求合并；跨过程容量耗尽仍由既有 1500 ms 定时刷新补偿，不宣称每次事件都有立即网络读。
- 排序缓存上限为执行工具预算 10000 + 1024 个额外条目；验证覆盖 1100 个待完成工具，不宣称任意无限模型轮次、异常超预算或淘汰后极晚结果的全局顺序。
- SQLite chat 派生表与 PostgreSQL display 派生表升级到 v2；canonical 记录不变，按访问线程以有界事务补建（128 项/约 2 MiB 批次）。已有数据库首次访问有补建成本，旧 v1 表暂不自动删除，磁盘会暂存两代索引；没有后台全库扫描，也没有证明迁移成本为零。
- 没有改线程 system prompt 冻结规则。测试必须先创建冻结 SessionMeta，再写 canonical 内容。

## 复验

Java 与 TLC 准备好后，在仓库外输出；所有参数为占位符：

```powershell
python scripts/check_work_process_formal.py --statechart <skill>/scripts/statechart.mjs --out <external-scratch>/work-process
python -m unittest discover -s tests -p test_work_process_formal.py -v
python -m unittest discover -s tests -p test_formal_proof_layout.py -v
cargo test -p wunder-runtime --lib change_stream_tests -- --test-threads=1
cargo test -p wunder-runtime --lib api::ws_helpers::tests -- --test-threads=1
cargo test -p wunder-runtime --lib thread_chat_summary -- --test-threads=1
cargo test -p wunder-runtime --lib canonical_thread_log_tests -- --test-threads=1
cargo test -p wunder-runtime --lib thread_change_hub -- --test-threads=1
cargo test -p wunder-desktop --lib chat_process::tests -- --test-threads=1
cargo test --manifest-path frontend-slint/Cargo.toml --bin wunder-frontend-slint cloud_ -- --test-threads=1
node frontend/scripts/run-regression-tests.mjs process-order-regression.test.ts thread-view-regression.test.ts process-refresh-gate-regression.test.ts
node frontend/scripts/run-vue-tsc.mjs --noEmit
```

浏览器在 `frontend/` 运行 `node ../node_modules/@playwright/test/cli.js test tests/e2e/timeline-process-stream.spec.ts tests/e2e/messenger-work-process-reasoning.spec.ts tests/e2e/thread-view-tail.spec.ts --workers=1 --output=<external-scratch>/browser`。

PostgreSQL 使用独立临时数据库并配置 `WUNDER_TEST_POSTGRES_DSN`，执行 `cargo test -p wunder-runtime --features postgres-storage --lib storage::postgres::admission_store::tests -- --ignored --test-threads=1`，以及 `postgres_execution_head_retains_status_identity_after_settlement` 和 `service_chat_` 的定向过滤。不要指向正在使用的数据。

TLC 1.8.0 jar SHA256：`78f7e2c6bf9a8b65eda665a23e94431255587e0b43a834a4e2af1723562d9b4a`。本轮 ProcessIdentity / ProcessDelivery 源 SHA256 分别为 `f31913e56c8fc784ce9997ecb13eb7dd90d1ed32455faab50001e78714786edc` / `82614c08320978e96cbb0c5101d878a43427f73a4f233d4b543f5ccda63c2b0d`；临时 TLC 状态和原始日志不入库，summary 的源键脱敏为 `scratch/`。

未覆盖：完整外部模型实机网络链路、Win7/Ubuntu 18.04 本轮平台实测、任意断线或多进程恢复协议、无限状态、真实滚动交互全覆盖，以及现有必做清单中其余未验收条目。
