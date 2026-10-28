# 服务长线程有限状态验证

## 契约与边界

使用 statechart 技能和 TLA+/TLC 对比修复前与修复后同一组规则。运行发生在仓库外 scratch，模型将生产的 500 个诊断槽抽象为 2 个、正文大小抽象为 1～3 个单位、单条预算为 2、投影条目身份为 3 个。`Commit` 表示 PostgreSQL 事务成功提交，失败/回滚是无状态改变；重复提交必须不增加计数。

模型依赖数据库事务原子性这一真实实现边界，不是对 PostgreSQL 内部 ACID 的证明。没有公平性/活性或时钟规则；没有覆盖真实并发吞吐、跨线程缓存总量、重建过程、网络请求合并和模型 API 时延。三类资源反例由对应生产测试与实测补强，不从小模型推出无限规模定理。

动作中的源码检查点已标文件与行号；`pre-fix HEAD` 表示本次修改前的仓库实现。实际代码可能因其他迭代移动，函数名为稳定定位依据：`MonitorState::append_event`、`SessionRecord::to_storage`、`thread_display::project_batch/counts`。

## 结果

`summary.json`：1400 个原模型状态、7440 条迁移；修复后未解决的图状态为 0，两个模型的未溯源条件均为空。所有十份 TLC 输出均包含 `Model checking completed`，无异常，检查队列耗尽。修复模型的四条规则全部通过；这是有界模型检查，不是参数化演绎证明。

| 规则/反例 | 图中最短编号步骤 | 旧模型 | 修复模型 | 实际验证 |
| --- | --- | --- | --- | --- |
| `BoundedDiagnostics` / bug 1 | 0 → 1 收到超预算事件 | 复现 | 不可达 | Unicode/转义/宽深嵌套投影及 600 次长事件单测 |
| `ConstantCountRead` / bug 2 | 0 → 1 提交投影 → 2 读统计 | 复现 | 不可达 | 真实 PG 增量计数、重放、补算、fork 与 EXPLAIN |
| `MetadataNoBodyCopies` / bug 3 | 0 → 1 收事件 → 2 读元信息 | 复现 | 不可达 | 真实元信息入口不返回事件，计费与轮次高水位保留 |
| `ExactProjectionCount` | 任意顺序重复提交相同/不同身份 | 通过 | 通过 | PG 重建、重复投影和事务 fork 回归 |

完整路径在 `stories.md`；总览 `chart.png`/`chart.mmd`，反例 `bug-1.png`/`bug-1.mmd`、`bug-2.png`/`bug-2.mmd`、`bug-3.png`/`bug-3.mmd`。PNG 已打开检查。`--cover` 仅压缩图的展示，不省略 TLC 探索，省略的图箭头列在 `summary.json`。

## 重跑

```powershell
python scripts/check_service_history_formal.py --statechart <statechart-skill>/scripts/statechart.mjs --out <external-scratch>/service-history-report
```

脚本复制 `docs/形式化证明/service-history-20261027/ServiceHistory.tla/.cfg/.map.mjs`，生成仅配置 `Fixed=TRUE` 不同的修复模型，然后执行 `from-spec --fixed --expect-broken BoundedDiagnostics,MetadataNoBodyCopies,ConstantCountRead --cover --png`。可设置 `JAVA`、`TLA2TOOLS` 使用预装工具。本次 Java 21，TLC jar SHA256：`936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`。

模型、配置、映射、TLC 完整文本输出与图均归档；日志中的机器绝对路径已经替换为 scratch 相对标识，未修改状态、反例、计数和结论。基准与测试命令详见 `../../性能基线/2026-10-27-服务长线程性能.md`。

生产入口回归输出：`postgres-tests.txt`（17 项，含第二次长历史压测）、`api-tests.txt`（SQLite/PostgreSQL 两项）、`monitor-tests.txt`（20 项）、`monitor-integration-tests.txt`（7 项）、`session-tests.txt`（16 项）。浏览器按需加载回归命令为 `node tests/trajectory-admin-timing.mjs`。
