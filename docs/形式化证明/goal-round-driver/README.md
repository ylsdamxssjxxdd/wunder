# 蛹目标轮次驱动验证：会话栅栏释放后的继续驱动

日期：2026-10-30。对象是蛹（`frontend-slint/` + `crates/wunder-desktop`）经原生流进入 `wunder-runtime` 的目标轮次驱动链；不覆盖蜂巢、舵机或兼容 bridge 的全量验收。

## 结论

现场现象是蜂巢的目标功能正常，蛹没有继续驱动智能体。根因不是目标领域状态损坏，而是**唤醒丢失**：目标在人类回合运行期间由模型通过 `goal` 工具创建/变更时，驱动任务被会话栅栏挡回；蛹的原生流要到消费完整个回合事件才 `drop(lease)`，而人类回合的 Direct-Run 路径释放租约后没有目标唤醒（只有目标轮与队列路径有释放后通知）。之后没有任何事件再请求驱动，活跃目标永久停在 `rounds_started = 0`。

修复后模型满足“活跃且武装的目标在空闲会话、未在轮中、未达轮次上限时必须继续开轮”；修复前的同一条规则被编号路径打破。

这不是“目标功能没有其他 BUG”的证明。模型是人工构造、与源码对应的有限抽象；只有回归测试覆盖到的边界可以据此验收。

## 已确认的问题与修复

| 编号 | 原始路径与违反的规则 | 实际证据 | 修复 |
| --- | --- | --- | --- |
| 1 | 人类回合运行中创建目标 → 驱动唤醒 → `host_session_available` 因原生流租约返回 false → 直接返回 → 回合结束 `drop(lease)` 无目标唤醒 → 目标停滞 | 现场数据：用户回合 14:25:07 开始，模型 14:25:16 创建目标（armed + active，`rounds_started = 0`），回合 14:25:37 结束，到 14:25:40 用户清除目标时 `rounds_started` 仍为 0；同一进程内改为空闲会话直接创建目标后，目标轮立即开始（`rounds_started = 1`） | `SessionLease` 绑定目标唤醒（`GoalLeaseWake`，`Weak<GoalService>`），释放时调用 `driver.notify_lease_released`；驱动在可用性探测**之前**置 `deferred`，只有被挡回过的驱动才在释放时重新请求，`WaitingUserInput`/`MaxTokens` 清空该标记以免越过“等待人工输入” |

## 模型与编号路径

- `models/GoalRoundDriver.tla` / `models/GoalRoundDriver.cfg`：修复前行为，租约释放没有目标唤醒。
- `models/GoalRoundDriverFixed.tla` / `models/GoalRoundDriverFixed.cfg`：修复后（当前代码），新增 `deferred`。
- 源码对应：`crates/wunder-runtime/src/services/goal/driver.rs:329`（探测前置 `deferred`）、`driver.rs:330`（可用性探测）、`driver.rs:200`（`notify_lease_released`）、`driver.rs:317`（在轮中跳过）、`crates/wunder-runtime/src/services/runtime/thread/runtime.rs:189`（`SessionLease::drop` 唤醒）、`crates/wunder-runtime/src/services/runtime/thread/runtime.rs:410`（目标轮准入）、`crates/wunder-desktop/native_stream.rs:238`（流泵结束才释放租约）、`crates/wunder-runtime/src/orchestrator/execute.rs:2271`（轮次结束唤醒）。
- `model/summary.json` 是统计来源：修复前模型 12 个状态、11 条迁移；对照图 12 个状态、13 条迁移；1 条修复前路径违反 `GoalKeepsDriving`，修复模型中同一规则满足，未标注源码来源的 guard 列表为空。
- `model/chart.png` 与 `model/chart.mmd` 是总图；`model/bug-1.png` 与 `model/bug-1.mmd` 是蓝边编号路径；详细故事见 `model/stories.md`。

编号路径为 **0 无目标且人类回合持有会话 → 1 回合中创建目标 → 2 驱动唤醒撞上忙碌会话 → 3 人类回合结束、释放栅栏**，终点即“活跃目标、0 轮、没有下一步”。修复把第 2 步变成“唤醒被记下”，释放时重新驱动；总图包含随后连续开轮到上限的完整路径。

模型刻意只描述单个目标、单个会话栅栏与一次释放：不包含多个控制者并发、目标轮与人类回合在队列中的交错、模型供应方故障、数据库失败/崩溃恢复、队列容量、跨进程租约和 UI 事件延迟。`deferred` 只表示“被挡回的驱动需要重评估”，不代表目标持久状态。

## 真实回归

- `crates/wunder-runtime/src/services/runtime/thread/runtime.rs` 新增 `goal_change_during_a_direct_run_drives_after_the_fence_release`：使用真实 `ThreadRuntime` 与真实 `GoalService`，先取得 Direct-Run 租约模拟运行中的人类回合，回合中创建目标并确认会话忙碌时不开轮（`rounds_started = 0`），释放租约后不再发送任何目标或轮次通知，验证目标轮自动开始。
- `crates/wunder-desktop/tests/goal_drive.rs::goal_keeps_driving_rounds_without_human_input` 实跑通过（`rounds_started >= 3`），验证蛹进程内的连续驱动。
- `cargo test -p wunder-runtime --features sqlite-storage --lib goal::driver` 15 项通过，含 `lease_release_re_drives_a_deferred_round` 与 `waiting_user_input_discards_a_deferred_drive`。
- 既有失败仍保留：`crates/wunder-desktop` 的 `native::stream::follow::tests::completed_history_is_drained_before_terminal_and_goal_updates_wake_settlement`（“active goal retains the completed follower”）本次未修改 follower，不宣称该扩展测试全绿。

## 复验命令

在仓库根目录的 PowerShell 执行测试：

```powershell
$env:RUST_MIN_STACK = '33554432'
cargo test -p wunder-desktop --test goal_drive -- --nocapture
cargo test -p wunder-runtime --features sqlite-storage --lib goal::driver -- --test-threads=1
cargo test -p wunder-runtime --features sqlite-storage --lib goal_change_during_a_direct_run -- --nocapture
```

模型需要 statechart 技能与已核验的 Java、TLC 1.7.4；可用 `JAVA` 和 `TLA2TOOLS` 指向本机安装，无需在仓库保存 Java/JAR。运行输出放仓库外：

```powershell
$proof = 'docs/形式化证明/goal-round-driver'
$skill = 'skills/statechart/scripts/statechart.mjs'
$out = Join-Path $env:TEMP ('goal-round-driver-' + [guid]::NewGuid().ToString('N'))
node $skill from-spec "$proof/models/GoalRoundDriver.tla" --fixed "$proof/models/GoalRoundDriverFixed.tla" --expect-broken GoalKeepsDriving --out $out --png
```

Windows + Node 24 上 `statechart` 的发布步骤对已存在的输出目录可能报 EPERM 或留下空目录，复验时使用新的 `--out` 路径即可；归档的 `model/` 是人工从运出目录复制并核对过的结果。

## 尚未证明的部分

- 任意并发、多个控制者、目标轮次与人类回合的队列交错不在模型范围内；蜂巢 WS 路径结构相同但未单独建模型。
- 未覆盖重启恢复、跨进程租约、目标预算计量与所有入口的行为等价。
- 本次证据不替代 Win7 32 位或 Ubuntu 18.04 的运行验证。
