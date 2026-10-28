# 聊天状态一致性验证

2026-10-28 续验：蛹与蜂巢的工作过程实时性、模型条目身份、稳定顺序和恢复证据见 [工作过程验证](./work-process-20261028/README.md)。下面的 600 ms 描述是 2026-10-26 历史快照；当前改为不重置首次期限的 120 ms 合并刷新，并增加有界实时条目投影，不再等待工具完成才展示。

验证日期：2026-10-26。对象：蜂巢聊天展示链路。蛹原生前端与舵机的补充审计、模型和回归见 `../蛹与舵机状态一致性验证/README.md`；舰桥尚未完成同等审计，各端范围不同，不能相互替代。

## 结论与范围

不能证明旧实现的线程条目、气泡和工具条目始终一致：TLC 找到了两个反例，真实 store 与 Vue 组件回放均复现。修复后，在相同有界模型中保持原规则重新检查，两个反例均消失；真实组件回放、SQLite 契约测试、启用 PostgreSQL feature 的独立数据库测试、WebSocket 唤醒测试和前端回归通过。

这是对指定事件交错的安全性和条件收敛验证，不是整个运行时的形式化证明，也不是毫秒级端到端时延保证。网络不送达、请求永久不返回、后台暂停、任意数量并发轮次及其他前端不在模型内。过程读取失败时显示错误，不用无限自动重试伪装收敛；仍保留原 600 ms 合并刷新窗口，另加请求和渲染耗时。

## 反例一：目录完成头先到

规则 `ImmediateTerminalAgreement`：已知某个执行轮次完成时，对应可见气泡不能仍处于运行态。

1. 受理后提交展示窗：线程与气泡均运行。
2. 目录先返回完成态：旧实现只更新执行头，线程条目完成，气泡仍运行。规则被打破。

真实回放观察到旧实现的 `thread=completed, bubble=running, task=done`。修复后同一步得到 `thread=completed, bubble=completed, task=done`。

修复：在核心 DTO、SQLite 与 PostgreSQL 展示页/变更/目录头和 WebSocket 最终帧中携带 `status_turn_id`。它表示状态所属的执行轮次，终态仍保留；`active_turn_id` 仍只表示活动轮次。前端按执行身份和水位投影匹配行，不猜测最后一个运行气泡，不推进正文重放游标；有界行级状态水位阻止迟到旧行重新打开气泡，更新的执行身份可以正常续跑。

证据：`head/bug-1.png`、`head/bug-1.mmd`、`head/chart.png`、`head/chart.mmd`、`head/stories.md`、`head/summary.json`。

## 反例二：终态刷新被在途读取吞掉

规则 `SettledToolAgreement`：完成态已到达，且没有待执行的系统刷新或读取时，可见工具状态应与已持久化结果一致。另保留 `NoFabricatedSuccess`：不得在工具真正完成前伪造成功。

1. 计数变化，安排刷新。
2. 刷新读取开始，抓到工具仍在运行的旧快照。
3. 工具结果落库。
4. 气泡收到完成态，再安排刷新。
5. 定时器触发，但读取在途；旧实现直接丢弃刷新。
6. 旧快照落地：气泡完成、工具转圈，已无请求或定时器，无法自行恢复。

旧实现真实回放观察到 `bubble=completed, tool=loading, pendingRequests=0, pendingTimers=0`。修复后旧响应结束恰好补读一次，结果为 `bubble=completed, tool=completed, pendingRequests=0, pendingTimers=0`。

修复：用一个布尔 dirty 闸门合并刷新需求。请求在途、初次读取期间、折叠期间或历史浏览期间均不丢需求；旧读取成功结束、重新展开或返回最新页后补读。失败不自动自旋；身份重置与卸载清理。工具状态仍取自真实结果，不把整轮完成等同于每个工具成功。

证据：`process/bug-1.png`、`process/bug-1.mmd`、`process/chart.png`、`process/chart.mmd`、`process/stories.md`、`process/summary.json`。

## 阅读状态图

- 蓝框和编号标记旧实现的反例路径。
- 旧实现单独图中的红色表示违反规则。
- 修复对照图的绿色表示旧错误状态已被消除，或修复模型新增的正确状态；绿色旧错误框不是说该错误现在合法。
- `stories.md` 写明首次偏离旧路径的位置；总览经过 `--cover` 裁剪，省略范围记录在 `summary.json`。

## 模型与代码的对应

`ThreadHead.tla` 和 `ProcessRefresh.tla` 描述修复前交错；其中代码行号对应修复前版本 `4df81500eefaf10668c3aaf6697e68e47fd46c2a`。`*Fixed.tla` 只改变所提修复的状态转移，使用同一配置和同一不变量，不削弱规则。

- 线程模型固定一个已绑定执行轮次，将服务端完成头作为外部输入；不建模完整目录传输、所有排队与续跑组合。其他轮次隔离、迟到行内容保留、新执行身份和失败/取消通过实际 store 回归补充验证。
- 工具模型固定一个可见工具、一次在途读取和有限结构事件；假设最新页可读、HTTP 成功且结果已提交。延时回调只作为事件顺序，不代表真实时钟。模型将读取结束后的 dirty 消费拆为两个系统步骤；生产代码在同一 `finally` 中连续执行。
- 真实组件回放编译原 `MessageTimelineTurnProcess.vue`，使用 Vue 自定义 renderer，通过点击、props、分页手势与可控 HTTP 响应驱动；仅替换网络、翻译和子展示组件，不复制生产状态逻辑。它验证可见工具状态及外部请求数量，不验证浏览器布局或真实网络性能。

## 复现命令

TLC/Java 在环境中可用后，从仓库根目录执行；工作产物写到仓库外的临时目录。`$skill` 指向本机 statechart 技能目录，`$scratch` 是新临时输出目录。

```powershell
$model = 'docs/形式化证明/聊天状态一致性验证/models'
node "$skill/scripts/statechart.mjs" from-spec "$model/ThreadHead.tla" --fixed "$model/ThreadHeadFixed.tla" --map "$model/ThreadHeadDelivery.map.mjs" --expect-broken ImmediateTerminalAgreement --out "$scratch/head" --png
node "$skill/scripts/statechart.mjs" from-spec "$model/ProcessRefresh.tla" --fixed "$model/ProcessRefreshFixed.tla" --map "$model/ProcessRefreshDelivery.map.mjs" --expect-broken SettledToolAgreement --out "$scratch/process" --cover --png
node frontend/scripts/regression/status-consistency-replay.mjs
cd frontend
node scripts/run-regression-tests.mjs thread-view-regression.test.ts process-refresh-gate-regression.test.ts timeline-activity-regression.test.ts task-list-regression.test.ts chat-thread-effects-regression.test.ts chat-session-merge-regression.test.ts
npm run typecheck:vue
cd ..
cargo test -p wunder-runtime --lib canonical_thread_log_tests -- --test-threads=1
cargo test -p wunder-runtime --lib api::ws_helpers::tests::display_watch_wakes_for_control_status_with_unchanged_rollout_cursor -- --exact
```

PostgreSQL 验证必须显式开启 `--features postgres-storage`，并将 `WUNDER_TEST_POSTGRES_DSN` 指向独立的一次性测试数据库；不能把 `running 0 tests` 算作通过：

```powershell
cargo test -p wunder-runtime --features postgres-storage --lib postgres_execution_head_retains_status_identity_after_settlement -- --ignored --test-threads=1
```
