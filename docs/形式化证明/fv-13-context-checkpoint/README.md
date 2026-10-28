# FV-13：上下文检查点的提交栅栏

日期：2026-10-27。状态：局部提交协议模型与真实回归，不是压缩全链路证明。

## 源头修复

蛹与舵机复用的两种压缩路径，在摘要生成后先检查取消，再经 `ThreadLogCommitter::commit_context_checkpoint` 提交。该入口与轮次结清共享同一线程提交闸：核验用户归属、原轮次仍为 `running`，持久追加检查点，最后发布变化。先结清后到达的摘要不能写入；检查点先获闸则先提交，终态写入随后执行。取消请求已出现但尚未结清不等于 durable 终态。

PostgreSQL live-chat 检查点在 canonical 追加事务内再次锁定并检查受理状态，与生命周期更新共用 advisory key；底层原始日志导入接口不增加该限制。SQLite 证明范围是共享 committer 的单运行时，不能扩展到绕开它的直接存储写入或多个独立进程。

压缩不再查询并注入最新长期记忆；原冻结 system prompt 保留，摘要构造移除遗留记忆区块。删除旧 fresh-memory 合并与查询实现，不以再次注入覆盖缓存前缀。正常压缩、失败回退都使用相同摘要清理逻辑。

## 模型与证据

`Checkpoint.tla` 与 `CheckpointFixed.tla` 共用 `Checkpoint.cfg`：一个原轮次、一个摘要、一次检查点、一次崩溃重开。闸门取得、状态核验、canonical 追加、调用返回、内存替换各为独立步骤，不把文件和数据库合成事务。

`model/summary.json`：24 个模型状态、45 条模型转换。旧模型违反 `NoLateCheckpoint`；修复模型保持该规则及 `DurableBeforeApply`、类型规则。一个反例类别的 numbered path 为：1 摘要就绪；2 旧路径绕过提交闸；3 未核验原轮状态；4 停止结清；5 迟到摘要仍写入。见 `images/bug-1.png`、`model/bug-1.mmd`、`model/stories.md`。

`checkpoint_rejects_late_summary_after_stop_without_changing_context` 使用真实 AppState、SQLite、JSONL 与 committer：先成功提交一个检查点，停止后再次提交被拒绝，历史游标、模型上下文和冻结 system 均不改变。它复现停止先于迟到提交的可观察后缀；不声称在真实文件 write 系统调用中注入停止。旧源码未在回滚工作区中重新构建。

## 真实回归

- `local_reduction_persists_canonical_checkpoint_without_rewriting_chat_history`：真实缩减与 canonical 恢复；当前输入从快照排除，再由自身消息追加一次；取消后的缩减返回 `None` 且恢复历史不变。
- `local_reduction_preserves_recent_groups_system_and_tool_protocol`：system、原生工具 call/result 对及最近工具组保留。
- `test_build_committed_replacement_history_from_rebuilt_strips_system_and_inflight_user`：完整压缩的当前输入与 system 去重。
- `test_compaction_summary_does_not_reinject_memory`、`test_compaction_summary_strips_legacy_memory_block`：摘要不重新注入长期记忆。
- `model/tlc/` 保存旧/修复同规则检查输出；总图为 `images/chart.png` 与 `model/chart.mmd`。

## 重现

将本目录 `.tla`、`.cfg`、`.mjs` 复制到仓库外 scratch，配置 `JAVA`、`TLA2TOOLS`、`STATECHART_SCRIPT` 后运行：

```powershell
node $env:STATECHART_SCRIPT from-spec Checkpoint.tla --fixed CheckpointFixed.tla --map Checkpoint.map.mjs --expect-broken NoLateCheckpoint --out model-output --cover --png
```

真实测试：`cargo test -p wunder-runtime --lib --no-default-features --features sqlite-storage compaction`，以及过滤 `checkpoint_rejects`。

## 限界

这里只证明完整检查点记录的进程崩溃恢复语义，不承诺断电 fsync 耐久性；JSONL checkpoint 和展示 item 不是不可分割的文件事务。自然语言摘要无语义损失、任意并发输入与工具结果、历史分叉/回退交错、所有磁盘失败点、手动压缩独立端到端链路、PostgreSQL 实库并发仍未覆盖。没有不可逆删除原始 canonical 历史。模型没有把文本结构与记忆查询抽象成伪证明，其保证来自明确列出的实际回归。
