# ThreadHead 终态与传输收尾续验证

日期：2026-10-28。对象：蜂巢聊天线程的请求收尾、持久终态、可见气泡和完成通知。

## 结论

修复前模型穷举得到 9 个状态、9 条转移和 5 类反例；固定模型保留同一状态空间和全部规则，`TypeOK`、`ImmediateTerminalAgreement`、`NoUnverifiedSuccess`、`TerminalDoesNotReopen`、`SameCutAgreement` 均通过，`chartStatus.wrong = 0`。

本轮根因是请求级 `final` 只表示传输结束，没有执行轮次身份，却被前端当作本轮完成。迟到的持久过程项和终态随后到达时，线程头、气泡和通知会短暂分叉；刷新会重新放大这条分叉。修复后，请求收尾统一为 `transport_final`，旧 `final` 在已接管线程视图的路径也只做传输收尾；完成通知必须同时具备 `sessionId`、`turnId` 和持久终态，并由线程头与对应气泡共同确认；同轮终态后到达的 `running` 快照只能补内容，不能重开状态。

## 反例与修复

| 编号 | 规则 | 旧路径 | 修复后 |
| --- | --- | --- | --- |
| 1 | `ImmediateTerminalAgreement` | 目录先完成，气泡仍运行 | 线程头与对应气泡一起完成 |
| 2 | `NoUnverifiedSuccess` | 无轮次身份的传输收尾触发成功提示 | `transport_final` 不产生完成提示 |
| 3 | `ImmediateTerminalAgreement` | 传输收尾后目录完成，仍保留运行气泡 | 仅持久终态确认完成 |
| 4 | `TerminalDoesNotReopen` | 同轮迟到 `running` 快照重开已完成气泡 | 终态水位拒绝重开 |
| 5 | `TerminalDoesNotReopen` | 传输收尾、刷新和迟到快照交错后重开 | 传输收尾不改执行状态，终态水位继续生效 |

详细路径见 [stories.md](./stories.md)，总图见 [chart.png](./chart.png) / [chart.mmd](./chart.mmd)，编号图见 `bug-1` 至 `bug-5`。

## 代码映射与边界

- 请求传输收尾：`crates/wunder-runtime/src/api/chat_ws.rs`、`crates/wunder-runtime/src/api/ws_helpers.rs`、`crates/wunder-runtime/src/api/chat/events.rs`。
- 传输事件归约：`frontend/src/stores/chatSendActions.ts`、`frontend/src/realtime/chat/chatCanonicalEvents.ts`、`frontend/src/realtime/chat/chatThreadRuntime.ts`、`frontend/src/utils/ws.ts`。
- 线程终态与气泡确认：`frontend/src/realtime/chat/threadViewStore.ts`。
- 完成通知：`frontend/src/views/messenger/controller/messengerControllerInstall.ts`，只接受带执行身份的持久终态事件。

模型固定一个执行轮次和一个可见气泡，抽象刷新、持久条目补齐和通知去重的关键交错；不覆盖任意数量并发轮次、永久断网、浏览器后台暂停、跨进程恢复、真实部署时延和完整网络重连协议。真实行为由前端定向回归、浏览器回放和 WebSocket 测试补充验证。

## 复验

在仓库外准备 TLC/Java 后执行：

```powershell
python scripts/check_work_process_formal.py --statechart <statechart-skill>/scripts/statechart.mjs --out <external-scratch>/work-process
python -m unittest tests.test_work_process_formal tests.test_formal_proof_layout -v
```

临时 TLC 状态、原始日志和完整报告不入库；本目录只保存脱敏摘要、故事、状态图和复验说明。
