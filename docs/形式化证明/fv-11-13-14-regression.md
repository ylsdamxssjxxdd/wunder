# 问询、上下文压缩、子智能体回归记录

日期：2026-10-27。本轮修改共享运行时及舵机问询接线，没有调整已停止日常维护的前端壳。

## 运行结果

| 检查 | 结果 |
| --- | --- |
| SQLite runtime lib 全量测试，同一最终测试二进制、4 测试线程 | 2153 通过、0 失败、4 忽略；38.50 秒，不含编译 |
| 默认高并发全量运行 | 2152 通过、1 失败、4 忽略；既有 `workflow_metrics_preserve_parallel_success_failure_and_cancellation` 的 1500ms 取消窗口使短命令未及时完成；单独复跑通过，未修改该无关测试 |
| `cargo check -p wunder-cli --bin wunder-cli --no-default-features --features sqlite-storage,postgres-storage` | 通过；包含本轮 PostgreSQL 受理、生命周期与检查点代码；舵机有 4 条既有 dead-code warning |
| 问询、压缩、子智能体旧/修复双模型 | 三组同规则检查通过；旧模型分别保留预期反例，修复模型没有这些违反；没有未映射 guard |
| 舵机新增面板身份单测 | 已编写，测试构建受既有临时轨迹截图模块缺失阻挡，不计通过 |
| PostgreSQL 实库 | 无隔离测试 DSN，未运行；编译通过不等于数据库事务已实测 |

全量结果还包含其他开发者并行工作区的测试，不能把总数全部记作本轮新增。模型统计与反例见各目录的 `model/summary.json`、`model/stories.md`，原模型与修复模型用同一份配置。TLA 注释里的 `storage/`、`orchestrator/`、`services/`、`ops/` 路径相对于 `crates/wunder-runtime/src/`；源码行号为本次证据建立时定位。

## 复现命令

```powershell
cargo test -p wunder-runtime --lib --no-default-features --features sqlite-storage -- --test-threads=4
cargo check -p wunder-cli --bin wunder-cli --no-default-features --features sqlite-storage,postgres-storage
cargo test -p wunder-cli --bin wunder-cli --no-default-features --features sqlite-storage inquiry_panel_preserves_consumption_identity --no-run
```

实际低并发复核直接运行刚由第一条同配置编译出的 runtime 测试二进制，避免与其他开发者抢占 Cargo 编译锁；测试代码与测试集合未裁剪。初次构建曾碰到其他并行修改的 cron 自由函数错误，随后对方更新后重新编译通过，没有在本轮修复无关逻辑。

TLC 使用本机已核验的 `tla2tools.jar`（SHA256 `936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`），运行输出自报 TLC2 2.19、Java 21。PNG 用 statechart skill 渲染。每组 README 给出在仓库外 scratch 重建全部图与验证的方法；不将任何机器绝对路径写入归档。

## 发布边界

本轮完成的是三组明确限界模型、对应生产修复和可运行回归。清单仍保留 FV-11/FV-13/FV-14 未整项勾选：多进程故障注入、实库交错、全部入口端到端、完整结果投递协议、摘要语义保真等不在已证明范围。没有新建无界队列/缓存，没有复制会话全文到 SQLite。
