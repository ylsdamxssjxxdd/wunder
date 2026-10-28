# FV-14：子智能体持久祖先的取消围栏

日期：2026-10-27。状态：局部派生/取消模型与回归；只针对现有子智能体，不恢复已移除的蜂群编排。

## 修复

主智能体临时创建的子工作单元可以由兄弟调度器继续派发。原实现只检查直接调度者是否取消，合并持久祖先后未再检查取消与深度，可能派入已经取消的分支。

现在 `MonitorState::register_child_run` 在和取消相同的 sessions 锁内检查调度者及全部持久祖先的取消记录，再进入 child registry；数据库读取仍在 monitor 锁外。`ChildRuns::register_with_ancestors` 在 registry 锁内核验合并祖先取消令牌，并在合并后核验 32 层上限与自环。保持原 8192 活动登记上限，没有新增取消墓碑缓存。

旧 guard 仍以 `Arc` 身份比较清理，不能删除同一线程的新代执行；完成的中间节点不使尚存后代脱离祖先取消范围。持久树的归属与回收继续复用现有 subagents tree/recovery 实现。

## 双模型

`ChildRuns.tla` 与 `ChildRunsFixed.tla` 使用 `ChildRuns.cfg`，限界为一个持久祖先、一个兄弟调度者、一个 worker 和两代执行。复用前必须已 settle，与实际登记 guard 一致。

`model/summary.json`：16 个模型状态、18 条模型转换。旧模型违反 `NoEscapedDescendant`，`LateGuardPreservesReplacement` 原本即成立；修复后全部配置规则通过，没有未映射 guard。编号反例：1 取消祖先；2 未取消的兄弟调度者向祖先分支派生任务。见 `images/bug-1.png`、`model/bug-1.mmd` 与 `model/stories.md`。

`cancelled_durable_ancestor_blocks_sibling_dispatch` 走真实 registry，`sibling_cannot_dispatch_into_a_cancelled_durable_ancestor_scope` 走真实 monitor 与 SQLite 持久树。两者均拒绝第二步；monitor 测试还验证祖先明确开启新执行后可以重新派发。原源码行为与模型对应，未回滚整个工作区执行旧二进制。

## 真实回归范围

- `durable_ancestry_respects_depth_and_cycle_boundaries`：合并后深度及自环拒绝。
- `child_reassignment_invalidates_old_wake_without_releasing_new_run`：旧清理迟到不删除新登记。
- `cancellation_covers_pending_nested_runs_and_allows_later_reuse`：嵌套取消、无关分支不受影响、后续复用。
- `child_directory_traversal_pages_and_preserves_independent_forks`：分页遍历不取消独立用户 fork。
- `task_tree_restores_identity_and_rejects_other_roots_users_and_cycles`：持久树归属。
- `expired_execution_recovers_once_preserving_history_and_fresh_workers`：过期执行幂等回收且保留历史、新 worker。

总图：`images/chart.png`、`model/chart.mmd`。`model/tlc/` 包含旧/修复运行输出。

## 重现与遗漏

复制 `.tla`、`.cfg`、`.mjs` 到仓库外 scratch，配置 `JAVA`、`TLA2TOOLS`、`STATECHART_SCRIPT` 后运行：

```powershell
node $env:STATECHART_SCRIPT from-spec ChildRuns.tla --fixed ChildRunsFixed.tla --map ChildRuns.map.mjs --expect-broken NoEscapedDescendant --out model-output --cover --png
```

真实测试：`cargo test -p wunder-runtime --lib --no-default-features --features sqlite-storage child_runs`，以及过滤 `subagents::lifecycle_tests`。

尚未覆盖任意树规模/多进程派发、结果重复投递的完整协议、取消记录和 registry 均已回收时的未来派生、关闭父线程的全部策略、物理 kill 延迟。模型不包含活性 PROPERTY；不能从安全不变量推出最终回收或公平调度。PostgreSQL 实库恢复尚未执行。
