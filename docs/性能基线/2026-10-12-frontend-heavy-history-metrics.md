# 2026-10-12 frontend 蜂巢重载历史渲染指标门禁

对应《蜂巢大数据量线程日志渲染优化方案》§5.2 W4-L「度量与门禁」。本报告记录指标名注册、记录接口、重载历史性能门禁与归档样张；真实 1000×1000 压测数字依赖 W1-A/B、W2-F 等上游工作项落地，不在本工作项范围。

## 目标

- 被测链路：蜂巢前端 `chatPerf` 采集面（`frontend/src/utils/chatPerf.ts`）与指标名白名单（`frontend/src/utils/chatPerfMetrics.ts`）。
- 风险点：白名单未注册的指标名会被 `chatPerfMetrics.ts` 的 `recordPerfDuration` / `incrementPerfCounter` 静默丢弃；插桩若用了未注册的名字，报告里看不到任何数字，也没有报错提示。

## 环境

- 代码状态：`main`（未提交；本轮同时有其他工作项在改，见文末「未决点」）。
- 机器：Windows 11，32 逻辑处理器。
- 运行方式：
  - 指标门禁：`node scripts/run-regression-tests.mjs chat-perf-heavy-history-regression.test.ts`（入参为裸文件名）。
  - 归档样张：同一入口加环境变量 `W4L_EMIT_REPORT=1`，测试把确定性样张报告打到标准输出。
  - 浏览器门禁：`npx playwright test tests/e2e/messenger-heavy-history-harness.spec.ts --workers=1`（需 `PLAYWRIGHT_BROWSERS_PATH` 指向已装浏览器的目录）。
- 关键配置：无。采样不读取任何消息文本、标识、路径或密钥。

## 采样方法

- 预热：无（采集面在 `chatPerf.start()` 前一概为空操作）。
- 数据规模：门禁样张为**确定性**输入，不代表真实线程规模。四个耗时指标各记 1 次：`snapshot_apply_ms` 184.5、`thread_projection_ms` 233.1、`virtual_window_recompute_ms` 12.4、`trajectory_layout_ms` 96.7；两个计数指标：`snapshot_abort` 记 2、`workflow_events_items` 记 25000。
- 重复次数：单次（门禁为确定性断言，不做多次采样）。

## 注册结果

白名单现包含本轮冻结的六个指标名，任何一个缺注册都会让门禁变红：

| 指标名 | 类型 | 记录接口 |
| --- | --- | --- |
| `snapshot_apply_ms` | 耗时 | `chatPerf.recordSnapshotApply(ms, meta)` |
| `thread_projection_ms` | 耗时 | `chatPerf.recordThreadProjection(ms, meta)` |
| `virtual_window_recompute_ms` | 耗时 | `chatPerf.recordVirtualWindowRecompute(ms, meta)` |
| `trajectory_layout_ms` | 耗时 | `chatPerf.recordTrajectoryLayout(ms, meta)` |
| `snapshot_abort` | 计数 | `chatPerf.countSnapshotAbort(delta)` |
| `workflow_events_items` | 计数 | `chatPerf.countWorkflowEventsItems(delta)` |

## 对比结果

门禁样张报告（`summary.heavyHistory`）：

| 指标 | 样张值 | 归档 | 结论 |
| --- | ---: | --- | --- |
| snapshotApply | 1 次 / avg 184.5 / p95 上限 200 | 有 | 注册生效 |
| threadProjection | 1 次 / avg 233.1 / p95 上限 500 | 有 | 注册生效 |
| virtualWindowRecompute | 1 次 / avg 12.4 / p95 上限 16 | 有 | 注册生效 |
| trajectoryLayout | 1 次 / avg 96.7 / p95 上限 100 | 有 | 注册生效 |
| snapshotAborts | 2 | 有 | 注册生效 |
| workflowEventItems | 25000 | 有 | 注册生效 |

负向对照（旧写法）在同一份断言里保留：

- 未注册名 `heavy_history_projection_probe_ms` / `heavy_history_probe_counter` 记录后**不出现在报告里**（旧行为，说明丢弃仍在发生）。
- 把六个名字从白名单移除再跑门禁：3 个断言转红（`snapshot_apply_ms must be registered` 等），确认门禁能因旧写法失败；随后还原，全部通过。

## 结论

- 判定：通过（对 W4-L 自身范围）。
- 理由：六个指标名已注册并可通过公开接口记录；未注册名仍被丢弃；回归门禁与浏览器门禁均可运行。
- 后续观察：真实 1000×1000 的六项数字要等上游插桩点落地后（见下）再复采，本报告不预设目标值。

## 建议插桩点（本轮不修改，属其它工作项）

以下文件本轮由其它工作项持有，W4-L 只回报不越界；落位时直接调用上表接口即可。行号为 2026-10-12 当日文件状态，实际落位以当时文件为准。

| 建议指标名 | 文件:行 | 位置说明 |
| --- | --- | --- |
| `snapshot_apply_ms` | `frontend/src/realtime/chat/chatThreadState.ts:715` | `applyChatThreadSnapshot`，快照应用的同步入口（分片入口在同文件 `:754`）。 |
| `thread_projection_ms` | `frontend/src/realtime/chat/chatThreadProjection.ts:84` | `buildChatThreadTurnSlots`，主线程轮次投影构建（W3-I 记忆化点）。 |
| `virtual_window_recompute_ms` | `frontend/src/views/messenger/messageViewportRuntime.ts:749` | `scheduleMessageViewportRefresh`，虚拟窗口重算调度（滚动入口在同文件 `:811`）。 |
| `trajectory_layout_ms` | `frontend/src/views/trajectory/trajectoryModel.ts:692` | `deriveTimeline`，轨迹时间轴布局派生（调用点在同文件 `:772`）。 |
| `snapshot_abort` | `frontend/src/realtime/chat/chatThreadRuntime.ts:411` | `dispatchSnapshotRequired`，失败/中止分支（调用点在同文件 `:488`）。 |
| `workflow_events_items` | `frontend/src/stores/chatRuntimeState.ts:697` | `loadSessionWorkflowEventsSnapshot`，workflow-only 事件装载（`workflow_only` 参数在同文件 `:716`）。 |

计数类建议在拿到响应体后按条数记：`snapshot_abort` 每失败一次 `+1`；`workflow_events_items` 每次装载 `+` 本次事件条数。


## 原始资料

- 归档样张：`assets/2026-10-12-frontend-heavy-history-metrics-report.json`（门禁确定性样张报告）。

## 未决点

- 真实压测数字未采：需 `VITE_DEV_PROXY_TARGET` + 后端压测线程，属上游工作项。
- 本轮工作区同时存在其它工作项改动；本报告只覆盖 W4-L 独占文件。
