# 数据库 I/O 与业务运行时隔离模型

本目录统一保存原测试模型目录中的 `PostgresIo` 有限模型。模型源码、配置和映射位于 `models/`，真实运行时回归仍保留在生产测试位置。

## 检查范围

模型以 4 个请求、2 个业务等待槽和 2 个替补等待槽抽象网络驱动与同步数据库调用的关系，检查 `NoIoStarvation` 与 `ExactCompletion`。`Fixed` 区分网络驱动是否独立于业务运行时，脚本在仓库外生成对应的修复模型。

2026-10-28 已实际执行 TLC 并打开检查两张 PNG。`results/summary.json` 记录原模型 912 个状态、2912 条迁移，修复后仍违反规则的图状态为 0；两份模型的未溯源条件为空。旧模型违反 `NoIoStarvation`，修复模型的两条不变量均通过。模型允许已全部完成时正常静止；“总能完成”没有公平性假设，不能从这里推出。

编号反例见 `results/bug-1.mmd`、`results/bug-1.png` 与 `results/stories.md`：调用同步数据库接口 → 移交业务线程 → 再调用并移交 → 替补槽耗尽 → 最后两个业务线程也等待数据库，驱动没有运行机会。实际 Compose 基线用 2000 个不同用户在线后发送 100 个任务，240 秒内完成 0 个、模型调用 0 次、舰桥探针超时，复现了该停滞；线程/数据库观测用于解释内部等待关系，不把外部压测说成逐指令调度复放。

真实回归 `postgres_io_uses_dedicated_runtime_from_async_and_blocking_callers` 检查同步桥接选用独立运行时；`postgres_queries_progress_when_caller_blocking_pool_is_saturated` 在 2 个业务 worker、2 个 blocking 槽下执行实际 PostgreSQL 查询，外部 15 秒看门狗验证查询可完成。前者检查实现选择，后者与真实 HTTP/WebSocket 压测检查可观察进展。

真实数据库并发、连接池、SQL 锁、OS 调度、网络故障和生产时延不由该有限模型直接证明；提交背压及显示降级后的队列终态由另行 Rust 回归和容量实测覆盖，未纳入此模型。

## 复验

```powershell
python scripts/check_postgres_io_formal.py --statechart <statechart-skill>/scripts/statechart.mjs --out <external-scratch>/postgres-io
```

可用 `JAVA`、`TLA2TOOLS` 指定已核验工具。脚本复制本目录模型到仓库外临时目录运行，完整结果按总索引的脱敏规则审查后再归档。
