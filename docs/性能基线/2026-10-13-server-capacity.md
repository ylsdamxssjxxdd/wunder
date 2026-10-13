# 舰体启动与后台维护容量回归

本次修复针对大库日常启动重复建索引、重复清洗历史数据，以及历史展示重建持续占用 CPU。没有删除部署数据。专项评分定义见 [性能评分体系](../性能评分体系.md#舰体容量专项评分)。

## 诊断依据

部署日志中，存储初始化用时约 41.9 秒，随后其他 AppState 初始化阶段约 230 ms。主要空档在时间索引准备阶段：代码每次启动都删除并重新创建工具日志和产物日志的时间索引，导致启动成本随表数据量增长。

只读诊断时，线程 item 分区约 300 万行、6.1 GB，工具日志分区约 200 万行、3.2 GB；线程目录只有 12 条。一个历史展示投影持续处于 rebuilding，数据库活动为搜索投影写入。一次 Docker stats 快照显示舰体 CPU 约 34%、数据库约 27%；这是瞬时证据，不能当作 15 分钟空闲均值。

启动慢与运行后 CPU 高来自两条链路：重复建索引拖慢启动；每页结束仅休息 20 ms 的重建循环持续生成正文、轨迹与搜索投影。大测试线程放大了缺陷，数据量本身不应使每次启动重复承担全量成本。额外发现长期记忆 valid_from 在启动时无条件 UPDATE 整表，已改为一次性清洗并只更新缺失值。

## 实现与验证范围

- schema 指纹与分区月份一致时，只读版本标记，跳过 DDL、数据清洗和索引重建；数据清洗另有持久化标记，其他 schema 变更不会重跑已完成清洗。
- 旧 BRIN 或无效时间索引只迁移一次，已有有效 BTree 保持；迁移在同 schema 的多实例间串行，错误不会把并发建索引失败无条件转成同步重试。
- 启动不发现历史展示头；后台每批按主键 seek 发现最多 64 线程，待迁移页最多 8 线程并持久化轮转游标。
- 维护延迟 10 秒启动，逐页串行，按实际工作耗时的 99 倍休眠且至少 1 秒；失败指数退避。SQL 超时与短锁等待限制数据库占用；每条 item 仍可能超过 40 ms 软时间片，40 ms 不是整事务硬上限。
- 管理运行时快照暴露维护工作时间、批次与失败计数，提供 Docker CPU 采样脚本。

## 隔离容量实测

环境为 Windows debug Rust 测试程序连接 Docker PostgreSQL 16，连接经本机映射端口。每次样本新建存储实例和连接池，库已完成 schema 初始化；空库和大库各 20 次，p95 使用 nearest rank。大库固定负载为 tool_logs、artifact_logs、thread_items、memory_fragments 各 100 万行，thread_logs 10 万行，全部为匿名 fixture 数据。

容量测试同时核对全部索引 OID、长期记忆行 xmin、启动后的展示头计数，验证旧 BRIN 的一次迁移和重启保持。真实异步维护循环处理百万 item 待重建线程，观察 60 秒窗口；必须推进游标并写出搜索投影，不能靠空跑取得低占用。

| 指标 | 本次结果 | 判定 |
| --- | ---: | --- |
| 空库存储初始化 p95 | 101.08 ms | 通过 |
| 大库存储初始化 p95 | 114.92 ms | 通过 |
| 容量放大附加 p95 | 13.84 ms | 通过 |
| 全部索引 OID 保持 | 是 | 通过 |
| 长期记忆行未重写 | 是 | 通过 |
| 启动展示头未发现 | 是 | 通过 |
| 历史发现/轮转/重启续跑 | 通过 | 通过 |
| 真实后台维护工作时间占比 | 1.018% | 通过（低于 2% 窗口红线） |
| 整套 x86 Compose 重启后舰体监听 | 1.754 s（server 容器启动到监听） | 通过 |
| 整套 x86 Compose 重启到全部依赖健康 | 108.75 s（含前端构建、OnlyOffice） | 通过 |
| 重启前后索引 OID 指纹 | 一致（394 个索引） | 通过 |
| 未完成投影大线程聊天窗口 p95（server） | 90.46 ms | 通过（S4） |
| 未完成投影大线程轨迹窗口 p95（server） | 39.25 ms | 通过（S4） |
| 未完成投影大线程聊天窗口 p95（nginx） | 76.03 ms | 通过（S4） |
| 未完成投影大线程轨迹窗口 p95（nginx） | 43.47 ms | 通过（S4） |

本次旧 BRIN 一次升级记录为 1736.27 ms。首次升级建新索引、添加字段或必要数据转换仍可能按数据规模耗时，单独保留 one_time_upgrade_ms。本次验证发现源窗口索引在旧大表上会触发约 59 秒一次性升级，已从启动 DDL 移除并改用既有索引；本次真实 x86 Compose 整套 stop/up 后，schema 快路径生效，存储 135.5 ms、AppState 483.9 ms、server 容器启动到 HTTP 监听 1.754 秒。整套到所有依赖健康为 108.75 秒，主要包含前端构建容器和 OnlyOffice 启动，不是 wunder-server 初始化耗时。接口复测经过 server 与 nginx 两个入口均通过。活动窗口的 CPU 快照仅作诊断记录，正式 15 分钟无请求门禁仍需单独采样。

## 回归命令与原始资料

```powershell
$env:WUNDER_TEST_POSTGRES_DSN = '<isolated key/value DSN>'
$env:WUNDER_CAPACITY_REPORT = [System.IO.Path]::GetFullPath('target/server-capacity-run.json')
cargo test -p wunder-runtime --features postgres-storage,sqlite-storage --lib postgres_capacity_startup -- --ignored --nocapture
python scripts/performance_score.py --input target/server-capacity-run.json --out-dir target/server-capacity-score
```

正式空闲采样和完整评分命令见评分体系，缺少 S2 时覆盖率为 75%，总分为空。实际用户和数据量继续增长时，启动路径仍只读取持久化 schema 指纹，展示维护按有界 keyset 批次、轮转游标和固定工作时间预算推进；首次升级、后台积压和数据库磁盘/连接容量仍需按增长量监控并重新采样。本轮百万级证据不能替代亿级实测，也不能保证所有在线查询的高并发延迟。

- 原始输入：[容量原始数据](2026-10-13-server-capacity-run.json)
- 自动评分：[容量评分报告](2026-10-13-server-capacity-score.json)
- 相关回归：wunder-core 94 项通过；runtime thread_view 7 项通过、1 项显式磁盘 benchmark 未运行；Python 评分与 CPU 采样 10 项通过；舰体 cargo check 通过。
