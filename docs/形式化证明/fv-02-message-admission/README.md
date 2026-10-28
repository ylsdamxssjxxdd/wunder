# FV-02 消息受理、重试去重与用户轮次分配

日期：2026-10-27。对象：共享引擎的消息受理链路（`canonical_turn_admissions` / `service_thread_admissions`）、排队任务派发闸门与崩溃恢复，SQLite 与 PostgreSQL 双存储实现。不是全量并发验收。

## 结论

`statechart` 对照确认：受理事务本身（单个 Immediate 事务内查重 → 分配轮次 → 插入受理行）的幂等与先写者胜语义成立，而崩溃窗口有两处真实缺陷——受理已提交但排队/执行续程随进程死亡的「孤儿轮次」永远停留在非终态（没有任何动作再驱动或结算它）；崩溃残留的 running/cancelling 监控记录永久挡住派发闸门，重启后排队中的已受理输入不会再被执行。两条规则在旧模型均如期违反，修复模型（孤儿结算 + 派发闸门锁活性仲裁）同一配置下全部通过。真实修复已落地并在 SQLite 真实存储上回归。

这不是"消息链路完全可靠"的证明。模型是人工构造、与源码对应的有限抽象（两条客户端消息、轮次限界 2）；只有回归测试覆盖到的边界可以据此验收。

## 已确认的问题与修复

| 编号 | 原始路径与违反的规则 | 实际证据 | 修复 |
| --- | --- | --- | --- |
| 1 | 受理提交 → 进程崩溃 → 重启：孤儿受理行（accepted/queued/running）无存活任务、无存活锁，永不结算；视图层仍把它投影成用户气泡，`execution_head` 永远报运行中。违反 OrphanedInputSettles | 旧模型反例 3920 条（最短 3 步：受理→崩溃→重启）；基线 `crates/wunder-desktop/runtime.rs:358-363` 启动只拉起队列循环，无孤儿结算 | `thread_admission.rs:606` `settle_orphaned_turns_in_tx`：无存活会话锁、无 pending/retry 任务、无新鲜 running 任务且超过 15s 宽限（`ORPHAN_SETTLE_MARGIN_MS`，≥打开租约 TTL）的非终态受理落 `interrupted`，`stop_reason=process_restart`，`input_json` 保留。触发点：`accept` 事务内（:531，查重之前）与桌面 `ensure_thread_open` 成功路径（`native.rs:709,745` 经 `ThreadRuntime::settle_thread_orphans` → committer 闸） |
| 2 | 崩溃残留的持久化监控记录停在 running：`is_session_idle` / `is_session_available_for_submit` 只看监控记录即判忙，重启后 pending 排队任务永不派发。违反 QueuedInputNotStarved | 旧模型反例 640 条（最短 7 步：K1 排队、K2 直跑持锁 → 崩溃 → 锁过期 → 重启后派发被残留监控永久挡住）；基线 `runtime.rs` 两处闸门无锁活性仲裁 | `runtime.rs:1347-1365` 与 `:1299-1345`：running/cancelling 监控记录只在会话锁存活时作数（执行必持锁，`execute.rs:224-233`，TTL=120s 心跳 5s；干净收尾先标 finished 再删锁行，不会误挡）；queued/waiting 合法无锁，不仲裁 |

附带清理：删除无调用者的旧恢复模块 `services/desktop_runtime_recovery.rs`（启动时按用户批量清锁/清任务/改监控，与"启动期间绝不清除其他实例的锁或任务"的现行契约直接矛盾），三处文档（`02-仓库结构与模块职责.md`、`11-Desktop-CLI本地形态设计.md`、`en/start/desktop.md`）同步为新恢复语义。

## 模型与编号路径

- `MessageAdmission.tla` / `MessageAdmission.cfg`：旧行为，行号参照基线提交 `508e12a` 的工作树。
- `MessageAdmissionFixed.tla` / `MessageAdmissionFixed.cfg`：修复行为，仅两处差异（新增 `SettleOrphan` 系统动作；`Claim` 去掉 `monitorStale` 闸门），行号参照本次修复后的源码；后续代码移动需更新引用。
- `MessageAdmission.map.mjs`：把 TLC 状态翻译为读者语言。
- `model/summary.json` 是统计来源：旧模型 9 次 TLC 运行、修复模型 7 次；合并对照图 33427 个状态、159573 条迁移（来自 27859 个模型状态）；works 13107、fixed 20320、wrong 0、worse 0；两条规则在旧模型如期违反，修复模型全部满足；未标注源码位置的 guard 列表为空。
- `images/bug-1.png`、`images/bug-2.png` 和 `model/` 同名 Mermaid 是两条规则的编号路径；详细故事见 `model/stories.md`。
- `model/chart.mmd` 是总图（33427 状态约 30MB，超过 Mermaid 渲染上限，无法出单张 chart.png；--cover 与否不影响状态数，因为图中没有 reader 动作）。

模型把锁 TTL/心跳抽象为布尔 lockLive、把 15s 结算宽限抽象为"恢复完成之后"（时延另测）；不建模 UserBusy 自旋、SystemBusy 限流、执行内的实际工作、多线程并发受理超过 2 键的情形。撤下（WithdrawTask→SettleWithdrawnTurn）两步之间的崩溃由 SettleOrphan 兜底为 interrupted 终态（与 cancelled 同为终态，正文保留）。

## 真实回归

全部在真实 SQLite 存储（tempfile 上的 `queue_1.sqlite` / `state_1.sqlite`）上运行：

- `storage/thread_admission.rs`（16 个测试全绿，新增 6 个）：
  - `concurrent_duplicate_accepts_share_one_round_and_one_row`——两个 `TurnAdmissions` 实例（等价两个本地进程）16 次并发同键受理，恰好 1 次 created:true、轮次恒为 1、受理行数 1。
  - `replay_after_lost_response_returns_first_identity_without_new_round`——响应丢失重发同键：created:false、轮次不变、正文保留。
  - `same_key_with_altered_content_keeps_the_first_write`——同键不同内容：先写者胜且可观察，存储正文不被替换。
  - `restart_settles_crash_window_orphan_and_keeps_dedup_identity`——崩溃窗口孤儿（受理后未入队、直接 SQL 回拨时间戳）在新实例重开后被结算为 interrupted、stop_reason=process_restart、正文保留、轮次不变；重复结算幂等；宽限内新鲜行不被触碰；同键重发仍命中去重。
  - `settle_respects_live_lock_and_live_or_unrelated_tasks`——存活会话锁挡住结算；pending 任务与新鲜 running 任务（领取→执行交接窗口）挡住；崩溃残留 running 行（时间戳过宽限）不再挡；链接他轮的任务不影响；终态行永不改动。
  - `settle_inside_accept_is_atomic_with_dedup`——accept 事务内先结算孤儿再查重，新键受理与孤儿结算原子生效。
- `services/runtime/thread/runtime.rs`：`stale_running_monitor_records_are_arbitrated_by_session_lock_liveness`——AppState 级（未启动派发循环）：无锁的 running 残留不挡提交/派发；存活锁 + running 记录仍判忙；锁行删除后残留记录不再挡。
- `storage/thread_lifecycle_tests.rs`：`sqlite_begin_turn_does_not_duplicate_the_user_message`——begin_turn 重放不重复物化用户消息（执行入口幂等）。
- 既有覆盖沿用：`queue_tasks.rs` 的 `interject_moves_one_turn_to_the_front_and_withdraw_drops_it`（排队后撤下终态）与 `thread_admission.rs` 的 `cancellation_without_rollout_preserves_pending_body_and_stays_terminal`。

PostgreSQL 实现（`storage/postgres/admission_store.rs` 的 settle 与 accept 内调用）与 SQLite 同语义同条件，本次仅编译期验证；运行时验证需要 `WUNDER_TEST_POSTGRES_DSN`，待补。

## 复现命令

在仓库根目录执行测试：

```powershell
cargo test -p wunder-runtime --features sqlite-storage --lib thread_admission
cargo test -p wunder-runtime --features sqlite-storage --lib sqlite_begin_turn
cargo test -p wunder-runtime --features sqlite-storage --lib services::runtime::thread
```

编译验证：

```powershell
cargo check -p wunder-runtime
cargo check -p wunder-runtime --no-default-features --features postgres-storage
cargo check -p wunder-desktop
```

模型需已安装 statechart skill；也可以用已有 Java 与经验证的 TLC 设置 `JAVA` 和 `TLA2TOOLS`，无需在仓库中保存 Java/JAR。运行输出放仓库外：

```powershell
$proof = 'docs/形式化证明/fv-02-message-admission'
$skill = Join-Path $env:USERPROFILE '.codex/skills/statechart/scripts/statechart.mjs'
$out = Join-Path $env:TEMP ('fv-02-' + [guid]::NewGuid().ToString('N'))
node $skill from-spec "$proof/MessageAdmission.tla" --fixed "$proof/MessageAdmissionFixed.tla" --map "$proof/MessageAdmission.map.mjs" --expect-broken OrphanedInputSettles --expect-broken QueuedInputNotStarved --out $out --cover --png
```

## 相关实现文件

- `crates/wunder-runtime/src/storage/thread_admission.rs`（宽限 :119；`settle_orphaned_turns_in_tx` :606；`settle_orphaned_turns` :675；`accept` 事务内结算 :531；受理线性化点 :520-599）
- `crates/wunder-runtime/src/storage/postgres/admission_store.rs`（同语义 settle 与 accept 内调用）
- `crates/wunder-runtime/src/storage/runtime_thread_store.rs`（`RuntimeAdmissions::settle_orphaned_turns` 双后端分发）
- `crates/wunder-runtime/src/orchestrator/thread_log_committer.rs:97`（结算走线程提交闸，与 accept_turn 串行）
- `crates/wunder-runtime/src/services/runtime/thread/runtime.rs`（`settle_thread_orphans` :690；闸门仲裁 :1299-1345、:1347-1365）
- `crates/wunder-desktop/native.rs`（`settle_orphans_best_effort` :655；触发点 :709、:745）
- 移除：`crates/wunder-runtime/src/services/desktop_runtime_recovery.rs`（死代码，语义被 F1/F2 取代）

## 尚未证明的部分

- PostgreSQL 运行时行为（环境无 PG，仅编译期验证同语义实现）。
- 宽限与 TTL 的物理时延：15s 结算宽限对"领取→执行交接"窗口的充分性是设计论证（交接在毫秒级刷新受理行），未做时延实测；多实例跨机时钟偏差下的宽限一致性未验证。
- 崩溃残留的 stuck 任务行本身仍留在 agent_tasks（状态 running）；它不再挡住受理结算与派发，但任务行的终态化清扫不在本项范围。
- 蜂巢（web）侧没有"打开线程"触发点；孤儿结算在 web 只靠 accept 事务内兜底（提交新输入时自愈），纯浏览不触发。
- 超过 2 键的并发受理、任意数量键的轮次交错；Model 轮次（模型轮次）分配不在本项范围（本项只覆盖用户轮次）。
