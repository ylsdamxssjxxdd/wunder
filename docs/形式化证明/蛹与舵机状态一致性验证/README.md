# 蛹与舵机状态一致性验证

验证日期：2026-10-26，增补 2026-10-27（线程状态相位、过程分页游标）。蛹是本地 AI 工作台，本次检查原生消息事件归约、过程分页、线程条目投影，以及窗口标题/侧栏的线程状态来源；舵机是手工本地操作入口，本次检查 TUI 前台/后台消息结算及 exec JSON 事件出口。蜂巢的独立审计见 `../聊天状态一致性验证/README.md`。

## 结论

旧实现不能满足所有检查规则，问题不是单纯刷新慢：取消或失败已经发生，工具仍可无限转圈；流订阅结束还可能把失败改成完成；目录旧快照也会把刚提交的本地任务误投影为完成；请求已经受理、工作过程仍在流式输出时，标题和侧栏仍能读成“空闲”；过程还有更早的步骤时，那个“查看更早的过程”按钮可能只重读同一页。六组 TLA+ 模型在旧转移中找到反例，修复模型使用同一配置和原不变量重新检查通过。生产入口回归也复现旧行为并验证修复，不能用状态图代替真实代码验证。

以下计数来自各目录 `summary.json`，不是生产场景数量：

| 模型 | 旧模型状态 | 反例规则数 | 修复对照图仍错误状态 |
| --- | ---: | ---: | ---: |
| `NativeSettlement` | 8 | 2 | 0 |
| `NativeProcess` | 2 | 1 | 0 |
| `CliSettlement` | 4 | 2 | 0 |
| `NativeCatalogueStatus` | 6 | 1 | 0 |
| `NativeStatusPhase` | 9 | 2 | 0 |
| `NativeProcessPaging` | 5 | 2 | 0 |

六个模型的 `TypeOK` 均通过，`uncited` 各项均为空。CLI 两条规则共用同一条反例路径，不能当作两个独立事件序列；`NativeProcessPaging` 的两条规则也共用同一条展开路径，分别说明“取不到”和“页丢了续读位置”。这里的“通过”仅指下述有限模型，不是任意并发正确性或端到端毫秒时延证明。

## 编号反例与修复

| 图中编号路径 | 被破坏的规则 | 真实旧行为 | 修复结果 |
| --- | --- | --- | --- |
| 蛹结算 1：①开始工具 → ②取消终态 → ③取消订阅结束 | `CancelledToolSettles`：本轮取消结算后工具不得继续转圈 | 工具仍为 `STATUS_RUNNING` | 结清为中断/非成功，不伪造工具成功 |
| 蛹结算 2：①开始工具 → ②失败终态 → ③迟到最终回复 | `TerminalOutcomePreserved`：迟到正文不得改写失败结论 | 气泡从失败被改为完成 | 拒绝失败终态后的迟到正文；完成或等待输入后的合法 final 仍可补齐答案 |
| 蛹分页 1：已有运行工具页 → ①完成历史替换运行历史 | `SettledProcessAgreement`：终态最新页在无系统刷新可执行时不得保留旧运行结果 | 已分页标志和旧工具页保留，live 定时器已停 | 更换页面模型身份，触发一次最新页读取，并拒绝旧身份响应 |
| 舵机结算 1：①开始工具 → ②失败终态 → ③订阅结束 | `FailedOutcomePreserved`：流关闭不得将语义失败改成完成 | 线程显示 `Finished` 而不是 `Failed` | 按线程保存终态，线程条目优先显示终态，Done 只结清订阅 |
| 舵机结算 2：同上路径 | `SettledToolAgreement`：订阅结束后未收到结果的工具不得继续转圈或伪造成功 | 普通工具卡仍显示 pending | 工具、命令、补丁及合并读取卡明确显示结果未知/非成功 |
| 蛹目录状态：①用户发送 → ②旧目录快照返回完成 | `StaleCatalogueCannotComplete`：旧目录快照不得结束本地刚提交的任务或弹完成提示 | 线程条目、气泡和完成通知把旧 `done` 当成真实终态 | 本地状态版本栅栏拒绝旧目录状态；真实执行终态仍可通知，后台线程仍可由目录完成通知 |
| 蛹状态相位 1：①发送消息（请求中） | `LiveNeverIdle`：工作过程还在跑时窗口不得显示空闲 | 标题计数按状态文案前缀猜，`请求中`/`模型响应中`/`上下文压缩中…`/`模型请求正在排队` 都落进 idle，标题显示“空闲”而非“运行中 N 线程” | 状态写入必须带相位（编译期强制），标题与侧栏只读相位；请求中即运行中 |
| 蛹状态相位 2：①执行失败 | `FailureShowsError`：失败终态不得显示为空闲 | 同一张前缀表没有失败族，`执行失败` 也归 idle，侧栏与标题都显示空闲 | 失败写入 `Phase::Error`，标题/侧栏显示错误，不再是空闲 |
| 蛹过程分页 1：①展开过程 → ②点“查看更早的过程” | `EarlierCanBeRead`：还有更早步骤且有可用入口时读者必须能取到 | 视图重建丢掉内存游标后，按钮请求实际读到最新页，屏幕不变；按钮因此“点了没用” | 移除按钮，滚动到顶即加载更早页；游标改由行持有 |
| 蛹过程分页 2：①展开过程 → ②视图被重建 | `LoadedPageKeepsItsEdge`：已加载的页必须保留续读位置 | 游标存在视图私有的 `HashMap` 里，页模型身份一变就被重置，`has_older` 仍为真 | `process-first`/`process-last` 随行保存，重建后仍能续读 |

补充发现与修复通过真实回归覆盖，但没有纳入上述 TLA+ 模型：

- 蛹不能靠错误文案是否含中文“失败”推导执行结论；现在用独立失败字段，错误详情与状态文案分离。
- 蛹保留显式 `native_execution_started` 的目标后继入口。运行时可能先发 durable terminal 再发 final，成功/等待输入终态后的 final 只能补正文，不能改状态；失败后 final 不再覆盖结论。
- 舵机 transport Error 后 producer 不再追加 Done；后台待重放 Error 不被 Done 替换，且不污染另一线程。
- 舵机 exec 识别 `turn_terminal`，重试中的 error 不直接算最终失败；关闭未报告结果的普通工具，避免命令兜底重复完成。
- 工具没有报告结果时，本轮结束不代表工具成功；子智能体是独立工作单元，其状态不随父轮强制结清。

## 图与源文件

蓝框与数字是旧实现的反例路径。修复对照图的绿色表示旧错误状态已不可达，或修复新增的正确状态；绿色旧错误框不是说错误现在合法。`stories.md` 给出首次偏离旧路径的位置，完整转移在 TLA+ 中。

| 范围 | 总览 PNG / Mermaid | 编号反例 PNG / Mermaid | 文字与统计 |
| --- | --- | --- | --- |
| 蛹结算 | `native-settlement/chart.png` / `native-settlement/chart.mmd` | `native-settlement/bug-1.png` / `native-settlement/bug-1.mmd`；`native-settlement/bug-2.png` / `native-settlement/bug-2.mmd` | `native-settlement/stories.md`、`native-settlement/summary.json` |
| 蛹分页 | `native-process/chart.png` / `native-process/chart.mmd` | `native-process/bug-1.png` / `native-process/bug-1.mmd` | `native-process/stories.md`、`native-process/summary.json` |
| 舵机结算 | `cli-settlement/chart.png` / `cli-settlement/chart.mmd` | `cli-settlement/bug-1.png` / `cli-settlement/bug-1.mmd`；`cli-settlement/bug-2.png` / `cli-settlement/bug-2.mmd` | `cli-settlement/stories.md`、`cli-settlement/summary.json` |
| 蛹目录状态 | `native-catalogue-status/chart.png` / `native-catalogue-status/chart.mmd` | `native-catalogue-status/bug-1.png` / `native-catalogue-status/bug-1.mmd` | `native-catalogue-status/stories.md`、`native-catalogue-status/summary.json` |
| 蛹状态相位 | `native-status-phase/chart.png` / `native-status-phase/chart.mmd` | `native-status-phase/bug-1.png` / `native-status-phase/bug-1.mmd`；`native-status-phase/bug-2.png` / `native-status-phase/bug-2.mmd` | `native-status-phase/stories.md`、`native-status-phase/summary.json` |
| 蛹过程分页游标 | `native-process-paging/chart.png` / `native-process-paging/chart.mmd` | `native-process-paging/bug-1.png` / `native-process-paging/bug-1.mmd`；`native-process-paging/bug-2.png` / `native-process-paging/bug-2.mmd`（另附 `-developer`/`-layered` 视图） | `native-process-paging/stories.md`、`native-process-paging/summary.json` |

`--cover` 对蛹结算图省略一条修复后从“失败后收到迟到回复”到“失败流已结束”的边，省略清单在该目录 `summary.json`；其他两图未省略状态或边。

`desktop-process.png` 是 Slint viewer 对真实 UI 组件的静态 fixture 渲染，已目视检查布局。它只证明组件可编译与静态展示可读，不能证明网络事件顺序或运行时状态正确。

## 代码对应与验证边界

- `models/*.tla` 是修复前模型，源行号对应 `4df81500eefaf10668c3aaf6697e68e47fd46c2a`；`*Fixed.tla` 对应本轮修复检查点。`NativeStatusPhase*` 与 `NativeProcessPaging*` 是 2026-10-27 增补，条件旁的文件与行号对应当前工作区的修复后检查点（旧实现的对应说法写在各动作上方的注释里）。`phase` 只区分有限事件位置，不是新增生产状态。迁移条件旁保留可打开的文件与行号，后续代码修改可能使行号移动。
- 蛹结算模型固定一个执行、一个工具，分别建模终态、迟到 final 和流结束。它没有覆盖所有状态组合、重试、任意多模型轮次、并发提交、云连接或目标后继；后继和合法 terminal→final 由真实归约器回归补充检查。
- 蛹分页模型固定一个展开的最新页，结果已持久化且读取成功。`RefreshTimer` 合并了初读定时器与成功响应两个系统阶段，不是逐原子步骤或真实时间模型；身份换代、拒绝旧响应和历史页不跳页由生产函数回归补充。读取失败明确展示错误，不承诺永久失败时自动收敛。
- 蛹目录状态模型固定一个线程和一次目录读取：目录读取可在用户提交前开始，随后返回旧完成状态。旧模型允许该返回覆盖本地热状态；修复模型用 `StatusProjection` 的本地状态版本和提交/执行所有权拒绝旧值，同时保留独立后台线程的 `hot -> done` 通知路径。
- 舵机模型固定一个线程、一次工具调用、一次语义失败和订阅结束。后台 replay、transport Error、等待用户输入、线程隔离、exec 与新提交清旧终态由生产入口回归补充，不属于 TLC 的证明范围。
- 蛹状态相位模型固定一条线程、一次请求：只区分空闲、排队、运行、完成、失败，一次只写一个短语。它没有覆盖多线程同时运行时的计数累加、切换线程后的重投影、云端行与本地行混排；这些由 `update_active_thread_filter` 与侧栏投影的真实回归补充。标题计数只在 `Phase::Running` 上加一，排队与等待输入计入“有任务”但不算运行中；这是产品口径，不是模型推出的结论。
- 蛹过程分页模型固定一个展开的最新页、一份仍存在更早步骤的存储。它合并了“展开”“首读”“视图重建”“点按钮/上滑”四个阶段，不建模读取延迟、并发读取、失败重试与真实滚动位置；读取失败的重试探针与滚动锚点在视图层，由编译检查与人工操作验收补充，不属于 TLC 的证明范围。
- 桌面伴侣（`companion_pet`）仍保留自己的一套宽松文本分类（含「确认」「审批」等短语），本轮没有并入相位表；它驱动的是宠物动画，不参与窗口标题与线程计数，因此不在本次证明范围，但同一条「按文案猜语义」的隐患仍在。
- 本地内存裁剪（实时流丢弃超量步骤）会把 `process_older` 置真而不带续读游标；此时上滑会降级为“重读最新页”，不是静默无反应。该降级路径不在模型中，需真实长过程验证。
- 本轮没有为舰桥建立同等模型，也没有穷尽蛹云线程、失联重连及任意并发。不得据此宣称所有形态的所有状态始终同步。
- 事件消费仍受调度与 I/O 耗时影响。蛹被动目录补查保留活跃约 500 ms、空闲约 3 s；运行过程最新页原刷新约 1.5 s，终态失效页使用约 33 ms 初读触发器；舵机轨迹页原刷新约 2 s。这些是触发间隔，不是延迟上限或“即时一致”承诺。
- 新状态字段每线程常数大小；过程保留单页，后台读取总并发上限沿用 4。没有增加无界事件队列、全历史拷贝或全量 DOM 重建。

## 真实回归结果

旧行为对照在仓库外的独立临时副本运行，保留新增测试，恢复对应旧分支；不是在当前工作区回退文件。观察到取消工具仍运行、英文错误未被判失败、失败后 final 改成完成、终态保留旧分页，以及 CLI 的 `Finished != Failed`、卡片 pending、exec 未关闭工具等断言失败。对照包含适配后的测试与字段，不能称为未经修改的旧版全量验收。

修复版定向验证：

- 蛹 `native_chat::turn_tests`：18 通过；显式跳过一个原有过程折叠断言失败项，新增六个终态回归均执行。
- 蛹 `chat_process::tests`：5 通过，包含分页失效/身份校验与历史页保留两个新回归。
- 舵机 TUI：五个新增回归通过，含实际持有活动订阅时失败终态立即优先的检查；调用真实 TuiApp 与卡片 renderer，不复制 reducer。
- 舵机 `exec_events::tests`：9 通过，包含两个新 exec 回归；`tui::thread_registry::tests`：9 通过，包含隔离、有界队列与压力回归。
- `slint-viewer --check` 通过；修改 Rust 文件自身的 `rustfmt --check` 通过，使用 `skip_children=true` 避免递归检查其他开发者文件；`git diff --check` 通过。
- 本次新增 `native_chat::status::tests` 6 项通过，覆盖旧目录回放、独立后台线程、执行中状态栅栏、结算后的下一次目录读取，以及失败/停止终态保护；使用真实状态投影逻辑，不创建通知窗口，因此不依赖测试机字体配置。

全量并非全绿：蛹本次 179 通过、8 失败、3 忽略；舵机的全量结果和失败清单见 `validation.md`。蛹相同八项失败也在旧行为对照运行出现：图标映射、Slint 平台跨测试线程初始化与过程折叠预期。舵机另有格式/别名、冻结提示词前写历史与线程打开租约等失败；未将它们计入本轮通过，也未扩大范围修复。

## 复现

从仓库根执行。`$skill` 指向本机 statechart 技能目录，`$scratch` 指向仓库外的新临时目录，Java/TLC 按技能配置可用后运行；不要将工具缓存或临时数据库归档。

```powershell
$model = 'docs/形式化证明/蛹与舵机状态一致性验证/models'
node "$skill/scripts/statechart.mjs" from-spec "$model/NativeSettlement.tla" --fixed "$model/NativeSettlementFixed.tla" --map "$model/NativeSettlement.map.mjs" --expect-broken CancelledToolSettles --expect-broken TerminalOutcomePreserved --out "$scratch/native-settlement" --cover --png
node "$skill/scripts/statechart.mjs" from-spec "$model/NativeProcess.tla" --fixed "$model/NativeProcessFixed.tla" --map "$model/NativeProcess.map.mjs" --expect-broken SettledProcessAgreement --out "$scratch/native-process" --cover --png
node "$skill/scripts/statechart.mjs" from-spec "$model/CliSettlement.tla" --fixed "$model/CliSettlementFixed.tla" --map "$model/CliSettlement.map.mjs" --expect-broken FailedOutcomePreserved --expect-broken SettledToolAgreement --out "$scratch/cli-settlement" --cover --png
node "$skill/scripts/statechart.mjs" from-spec "$model/NativeCatalogueStatus.tla" --fixed "$model/NativeCatalogueStatusFixed.tla" --map "$model/NativeCatalogueStatus.map.mjs" --expect-broken StaleCatalogueCannotComplete --out "$scratch/native-catalogue-status" --cover --png
node "$skill/scripts/statechart.mjs" from-spec "$model/NativeStatusPhase.tla" --fixed "$model/NativeStatusPhaseFixed.tla" --map "$model/NativeStatusPhase.map.mjs" --expect-broken LiveNeverIdle,FailureShowsError --out "$scratch/native-status-phase" --png
node "$skill/scripts/statechart.mjs" from-spec "$model/NativeProcessPaging.tla" --fixed "$model/NativeProcessPagingFixed.tla" --map "$model/NativeProcessPaging.map.mjs" --expect-broken EarlierCanBeRead,LoadedPageKeepsItsEdge --out "$scratch/native-process-paging" --png
cargo test --manifest-path frontend-slint/Cargo.toml --bin wunder-frontend-slint native_chat::turn_tests -- --test-threads=1 --skip one_tool_round_becomes_grouped_entries_and_frees_the_next_round
cargo test --manifest-path frontend-slint/Cargo.toml --bin wunder-frontend-slint native_chat::status::tests -- --test-threads=1
cargo test --manifest-path frontend-slint/Cargo.toml --bin wunder-frontend-slint chat_process::tests -- --test-threads=1
cargo test -p wunder-cli --bin wunder-cli survives_the_stream_done_marker -- --test-threads=1
cargo test -p wunder-cli --bin wunder-cli background_error_is_not_replaced_by_done_before_replay -- --test-threads=1
cargo test -p wunder-cli --bin wunder-cli waiting_terminal_is_preserved_and_new_submission_clears_old_failure -- --test-threads=1
cargo test -p wunder-cli --bin wunder-cli stream_end_never_leaves_tool_cards_running -- --test-threads=1
cargo test -p wunder-cli --bin wunder-cli exec_events::tests -- --test-threads=1
cargo test -p wunder-cli --bin wunder-cli tui::thread_registry::tests -- --test-threads=1
$env:SLINT_ENABLE_EXPERIMENTAL_FEATURES = '1'
slint-viewer --check frontend-slint/ui/main.slint
slint-viewer --screenshot "$scratch/desktop-process.png" --size 960x700 frontend-slint/ui/chat_alignment_demo.slint
```
