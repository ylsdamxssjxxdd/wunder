# FV-11：问询回答的持久消费身份

日期：2026-10-27。状态：局部模型与真实修复回归，不是 FV-11 整项验收。

## 契约与修复

蛹的问询回答是一个新的用户轮次；旧等待轮次另行结清，不把新消息挂到旧 root。消费作用域是同一用户、同一任务线程中的 `inquiry_turn_id`。同一问询的不同客户端消息键只能得到同一个回答身份，先写者胜，不修改首个回答正文。

SQLite Immediate 事务与 PostgreSQL 线程 advisory lock 事务同时写入新回答和旧问询的 `answered_by_turn_id`。原问询仍是 `waiting_input`，随后经过统一 `ThreadLogCommitter` 追加 canonical 终态并更新控制面。两步不是一个原子操作；失败后重试可以补结算，不得在受理时伪造 canonical 完成。终态物化输入也不再把等待轮次临时改回 `running`。

统一提交边界服务于排队入口和直接引擎入口；舵机问询面板现在保留事件中的原轮次身份，选择回答与自由文本回答均传回该身份。非法身份、跨线程目标、非等待目标、伪造结算身份被拒绝。先停止后回答不分配新轮次；回答先受理后停止，保留已受理身份但不把取消态改回完成。

## 模型与反例

旧模型 `Inquiry.tla` 与修复模型 `InquiryFixed.tla` 使用相同 `Inquiry.cfg`。限界为一个问询、两个不同客户端键、一次进程崩溃及重启；不建立公平性或最终自动执行的活性结论。

`model/summary.json`：162 个模型状态、292 条模型转换，两个反例类别；修复后两条业务规则及类型规则通过，guard 均有源码映射。统计是工具图口径，不是生产请求量或漏洞数量。

| 图与规则 | 编号路径 | 真实回归与结论 |
| --- | --- | --- |
| `images/bug-1.png`、`model/bug-1.mmd`；`AtMostOneAnswer` | 1 提交回答；2 另一客户端再提交 | `inquiry_concurrent_clients_share_one_durable_consumption_identity` 使用两个连接并发进入真实受理事务；只产生一个身份与一个新用户轮次。旧源码与模型对应，未回滚整个工作区执行旧二进制。 |
| `images/bug-2.png`、`model/bug-2.mmd`；`StopWins` | 1 停止问询；2 提交回答 | `inquiry_stopped_before_answer_cannot_allocate_a_new_user_round` 通过真实 committer 重放路径，拒绝受理且轮次不变。 |

完整图为 `images/chart.png`、`model/chart.mmd`，详细步骤见 `model/stories.md`。`model/tlc/` 保存去除机器路径的 TLC 输出。

## 回归入口

- `storage::thread_admission::tests::inquiry_answer_is_consumed_once_across_distinct_clients_and_reopen`：重新打开存储后仍去重。
- `orchestrator::thread_log_committer::tests::inquiry_claim_survives_interrupted_settlement_and_rejects_forgery`：停在受理事务后，再经统一入口重试补结算；不是强杀进程测试。
- `orchestrator::thread_log_committer::tests::inquiry_unmaterialized_waiting_turn_keeps_waiting_state_until_completion`：尚未物化的输入经 canonical 终态结算后，控制面和页面均为完成。
- `orchestrator::request::tests::direct_inquiry_answer_settles_waiting_turn_and_deduplicates_other_clients`：舵机使用的直接引擎请求准备链路，不调用真实模型。
- `storage::canonical_thread_log_tests::inquiry_answer_starts_new_user_round_and_keeps_original_question`：原问题、新回答、用户轮次及面板投影。
- 舵机新增 `inquiry_panel_preserves_consumption_identity_for_direct_and_nested_events`；测试构建仍受既有缺失的临时轨迹截图模块阻挡，不能记为运行通过。

## 重现命令

先设置 `TLA2TOOLS` 与 `JAVA`，使其指向本机已核验的工具。将本目录的 `.tla`、`.cfg`、`.mjs` 复制到仓库外 scratch，再在该目录运行（`STATECHART_SCRIPT` 指向 statechart skill 脚本）：

```powershell
node $env:STATECHART_SCRIPT from-spec Inquiry.tla --fixed InquiryFixed.tla --map Inquiry.map.mjs --expect-broken AtMostOneAnswer --expect-broken StopWins --out model-output --cover --png
```

仓库根目录运行 `cargo test -p wunder-runtime --lib --no-default-features --features sqlite-storage inquiry`。

## 未覆盖

PostgreSQL 真实数据库运行、跨进程故障注入、网络响应丢失与所有端到端面板交互仍待验收；当前没有孤立数据库 DSN。未绑定身份的普通文本仍是普通新消息，不从历史猜测目标。受理后既不重试也不访问恢复入口时，不保证旧问询立即补结算；执行续程按既有孤儿轮次策略处理，不能把去重等同于外部副作用恰好一次。
