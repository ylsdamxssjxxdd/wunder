# FV-04 停止、取消、超时与执行结清

日期：2026-10-26。对象：共享引擎的停止管线（`cancel_session_activity`）与执行体收尾结清（`finish_request_success` / `finish_request_error` / 手动压缩结清），覆盖持久控制面（`thread_turns` admissions + canonical）与呈现面（监视器、终态帧、goal 结局）的一致结清。不是全量并发验收。

## 冻结契约

停止的生命周期固定为四个阶段，任何交错下都不得越序或遗漏：

1. **停止请求**：入口（REST `chat.rs:762-767`、WS `chat_ws.rs:1059-1077`、桌面 `native_stream.rs:292-296`）只发起请求，不做结清。
2. **取消已受理**：执行围栏置位（`execution.interrupted=true`）→ 监视器置 `cancelling`（仅活跃态受理）→ 活跃轮次写 `cancelled`（终态不可逆）。此后执行体的取消检查点必然可见。
3. **执行已隔离**：执行体在检查点看到取消走 `CANCELLED` 结清；收尾提交被持久面拒绝时必须改按取消语义结清；超时按失败结清。完成提交先于取消到达则完成成立。
4. **外部命令结束**：进程 token 熔断 → 命令循环 kill 子进程；执行体已不在时由取消方补写监视器终态，但 `finished`/`error` 终态记录不得改写。

可接受语义：已生成内容保留；停止幂等（重复停止不再取消任何轮次）；结清后可再发言（register 重置取消标志、新租约新 token，旧 token 保持熔断）。

## 结论

`statechart` 对照确认：旧模型四条规则中两条真实违反——「取消已受理后呈现面不得出现完成语义」与「平息后轮次终态与监视器终态同语义」；两条目标规则（停止能结清、结清后能再发言）如期给出反例路径。修复模型同一配置下两条真实违反全部消除（`fixed`，无 `worse`），目标规则保持原反例。真实修复已落地，并在挂起模型、挂起工具（审批等待）与停止晚于完成三条真实流式链路上回归。

这不是"停止语义完全正确"的证明。模型是人工构造、与源码对应的有限抽象（一个执行体、一条停止管线、至多一个外部命令）；只有回归测试覆盖到的边界可以据此验收。

## 已确认的问题与修复

| 编号 | 原始路径与违反的规则 | 实际证据 | 修复 |
| --- | --- | --- | --- |
| 1 | 停止受理落在执行体最后一个取消检查点之后、`finish_request_success` 之前/之中：持久面拒绝完成写入（轮次保持 `cancelled`），但仍发 `turn_terminal{completed}` + `final` 帧、自动记忆与 goal Completed | 旧模型反例 4 条（最短 4 步：`PassLastCheckpoint`→停止→受理→`CancelTurns`→`FinishSuccess` 拒绝但仍发完成语义）；基线 `execute_tools.rs` 旧 `finish_request_success` 无取消前置检查且丢弃 `update_turn` 的 `Ok(false)` | `execute_tools.rs:205-207` 收尾前最后一道取消闸（`ensure_not_cancelled`）；`:221-241` 采纳 `update_turn` 返回值，`Ok(false)` 即返回 `CANCELLED`，由外层路由进 `finish_request_error` 的取消结清。同构修复：`memory.rs:1787-1797`（手动压缩结清前置闸）与 `:1816-1841`（`Ok(false)` 改按取消语义并尽力把轮次落为 `cancelled`） |
| 2 | 监视器 flap：收尾把 `cancelling` 改成 `finished` 后无人修复（队列路径 `is_task_cancelled` 提前返回跳过 `mark_cancelled`，尾部补写因执行体活跃被跳过） | 旧模型规则2 反例 6 条之一；基线 `runtime.rs:1828-1832` 提前返回、`:858-863` 尾部补写条件不成立 | 随编号1修复：拒绝完成写入后不再执行 `mark_finished`，flap 源头消除 |
| 3 | 尾部补写无终态守卫：停止晚于完成/失败到达时，`mark_cancelled` 把监视器记录从 `finished`/`error` 改写为 `cancelled` | 旧模型规则2 反例最短路径：超时失败结清→停止→尾部补写改写；基线 `monitor.rs` 旧 `mark_cancelled_with_source` 无条件改写 | `monitor.rs:1295-1301`：`finished`/`error` 终态记录不再改写为 `cancelled` |

## 模型与编号路径

- `StopCancel.tla` / `StopCancel.cfg`：旧行为，行号参照基线提交 `508e12a`。
- `StopCancelFixed.tla` / `StopCancelFixed.cfg`：修复行为，行号参照本次修复后的源码；后续代码移动需更新引用。
- `StopCancel.map.mjs`：把 TLC 状态翻译为读者语言。
- `model/summary.json` 是统计来源：旧模型 54 个状态、100 条迁移；合并对照图 41 个状态、40 条迁移；works 30、wrong 3、fixed 14、worse 0；`CancelNeverPresentedAsCompleted` 与 `SettledViewsAgree` 在旧模型如期违反、修复模型全部满足，目标规则 `NeverSettlesCancelled`（停止能结清）与 `NeverReopens`（结清后能再发言）在旧模型给出反例路径且修复模型保持；未标注源码位置的 guard 列表为空（两个模型的 `uncited` 均为空）。
- 对照图的 3 个 wrong 状态全部是两条**目标规则**的有意反例终点，不是缺陷：`s0`（已结清并重开新轮次，违反 `NeverReopens`）与 `s5`/`s6`（停止管线走完后轮次/监视器/终态帧/goal 均为取消语义的平息态，违反 `NeverSettlesCancelled`）——它们正是"停止能结清、结清后能再发言"的证据。真实缺陷的违反终点（已停止轮次仍发完成语义、已失败/已完成轮次被改写成已停止等轮次与监视器失配状态）全部被归为 fixed，即修复模型不可达。
- 限界机制说明：模型只刻画一个执行生命周期。重开由 `Reopen` 把停止管线置为 `over` 关闭态实现（而非全局布尔门控），重开后不再有本生命周期的动作；`NeverReopens`/`NeverSettlesCancelled` 是为取得反例路径而故意倒置的目标规则。
- `images/chart.png` 与 `model/chart.mmd` 是总图；`images/bug-1.png` 至 `bug-4.png` 和同名 Mermaid 是四条规则的编号路径；详细故事见 `model/stories.md`。

模型把超时抽象为一个结清动作（时延另测），不建模网络细节、多轮工具循环与多执行者并发；限界为单执行体、单停止管线、至多一个外部命令。

## 真实回归

回归入口是 `crates/wunder-runtime/src/orchestrator/thread_lifecycle_tests.rs`：

- `stop_while_model_call_hangs_settles_cancelled`：模型 stub 永不响应，等执行体注册 running 后真实调用 `cancel_session_activity`；断言 CANCELLED 错误帧与 `turn_terminal{cancelled}`、无 `final` 帧、轮次 `cancelled`、`thread_view` runtime `cancelled`、监视器 `cancelled`。
- `stop_while_tool_waits_for_approval_settles_cancelled`：`approval_mode=suggest` 下模型要求 `write_file`，等审批请求到达后停止；断言同上结清语义，且无 `workspace_update`（工具未执行）、模型调用计数为 1（取消后没有新一轮模型调用）。
- `stop_after_completion_keeps_completed_settlement`：即时应答跑完再停止；断言 `turns_cancelled == 0`、轮次保持 `completed`、监视器保持 `finished`（编号3修复不误伤）、回复气泡完整。
- `finish_request_success_honours_turn_cancelled_before_commit` / `finish_request_success_refuses_after_cancel_accepted`：直接调用修复点函数，分别覆盖「持久面拒绝完成写入」与「前置取消闸」两种形态；断言返回 `CANCELLED`、轮次保持 `cancelled`、监视器不被改写为 `finished`。旧实现下这两个测试返回 `Ok` 且监视器 flap 为 `finished`，即缺陷复现。

## 复现命令

在仓库根目录执行测试：

```powershell
cargo test -p wunder-runtime --features sqlite-storage --lib thread_lifecycle_tests
```

模型需已安装 statechart skill；也可以用已有 Java 与经验证的 TLC 设置 `JAVA` 和 `TLA2TOOLS`，无需在仓库中保存 Java/JAR。运行输出放仓库外：

```powershell
$proof = 'docs/形式化证明/fv-04-stop-cancel-settlement'
$skill = Join-Path $env:USERPROFILE '.codex/skills/statechart/scripts/statechart.mjs'
$out = Join-Path $env:TEMP ('fv-04-' + [guid]::NewGuid().ToString('N'))
node $skill from-spec "$proof/StopCancel.tla" --fixed "$proof/StopCancelFixed.tla" --map "$proof/StopCancel.map.mjs" --expect-broken CancelNeverPresentedAsCompleted --expect-broken SettledViewsAgree --expect-broken NeverSettlesCancelled --expect-broken NeverReopens --out $out --cover --png
```

## 相关实现文件

- `crates/wunder-runtime/src/orchestrator/execute_tools.rs`（`finish_request_success` 取消前置闸 :205-207、采纳持久写入结果 :221-241；`finish_request_error` 取消结清 :62-178）
- `crates/wunder-runtime/src/ops/monitor.rs`（`mark_cancelled_with_source` 终态守卫 :1295-1301；`cancel_with_source` 活跃态受理 :1345-1351）
- `crates/wunder-runtime/src/orchestrator/memory.rs`（手动压缩结清同构修复 :1787-1797、:1816-1841）
- `crates/wunder-runtime/src/orchestrator/thread_lifecycle_tests.rs`（回归测试）
- `crates/wunder-runtime/src/services/runtime/thread/runtime.rs`（`cancel_session_activity` 停止管线 :675-891；尾部补写条件 :858-863）
- `crates/wunder-runtime/src/orchestrator/llm.rs`（`ensure_not_cancelled` :330-343、`wait_for_cancelled` :345-394）
- `crates/wunder-runtime/src/orchestrator/thread_log_committer.rs`（`update_turn` 守卫 :221-266、`cancel_active_turns` :300-340）、`thread_admission.rs`（终态不可逆 :622-635）

## 尚未证明的部分

- 多执行者并发（队列中后续任务、子智能体树）与停止的交错；模型限界为单执行体。
- 外部命令超时 kill 的物理时延与残留进程；模型只断言结清后无存活命令。
- 取消窗口内已开始流式输出的 token 级一致性（按帧合并、不丢字属渲染层语义）。
- 手动压缩结清路径只有源码同构修复与单元级核查，未加独立的真实回归（其取消闸与 `Ok(false)` 分支和 `finish_request_success` 同模式）。
