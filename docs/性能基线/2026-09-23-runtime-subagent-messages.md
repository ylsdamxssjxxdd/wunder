# 2026-09-23 运行时子智能体消息性能基线

## 目标

验证运行中 mailbox、SQLite/PostgreSQL 持久消息入队，以及停止、恢复、指导、汇报链路的正确性和并发性能。

## 环境与方法

- Release 编译：Rust 1.95 MSVC，`-j 8`；Windows x64，16 核/32 线程。
- mailbox：1/2/4/8 工作线程，每档 20,000 条，发送后立即消费并核对。
- 入队：1/2/4/8 并发，每档 200 条；SQLite WAL/NORMAL，PostgreSQL 16 独立 Docker 实例。
- 完整链路：隔离 AppState/SQLite 实例，本地模拟模型，覆盖停止、恢复、重复消息、冻结 system prompt、report 和过期 completion。

## 结果

### SQLite 入队（连接复用后）

| 并发 | 吞吐 msg/s | p50 us | p95 us | p99 us |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 8770.0 | 48 | 80 | 4105 |
| 2 | 10262.7 | 54 | 782 | 3590 |
| 4 | 10707.1 | 52 | 1921 | 5600 |
| 8 | 9254.7 | 54 | 5611 | 10919 |

连接复用前基线：1 并发 195.8 msg/s、p50 5012 us；8 并发 518.9 msg/s、p50 3332 us、p99 202515 us。

### PostgreSQL 入队

| 并发 | 吞吐 msg/s | p50 us | p95 us | p99 us |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 136.7 | 7278 | 7912 | 8518 |
| 2 | 270.2 | 7343 | 8066 | 9189 |
| 4 | 520.0 | 7610 | 8420 | 9509 |
| 8 | 967.0 | 8162 | 9327 | 10397 |

### Mailbox 与完整链路

- mailbox：1/2/4/8 档约 664k/1.16M/2.36M/2.83M msg/s；p99 2.0/1.9/2.9/5.4 us。
- 完整链路：1/2/4/8 档 2463/1152/3255/5640 ms，全部通过；独立进程采样峰值约 175 MiB。

## 正确性

- SQLite、PostgreSQL 原子容量、重复 ID、冲突 ID、running 任务不被覆盖通过。
- 子线程停止递归取消、保留历史、同线程 resume/send、消息顺序和 system prompt 冻结通过。
- 过期 completion 只标记自身 `not_applied`，不取消父线程或后续 report。

## 限制

PostgreSQL 完整链路在 Docker checkpoint 写入压力下曾超过测试等待窗口；数据库消息入队、PostgreSQL 专项回归和本地完整链路均通过。压测使用本地模拟模型和隔离数据库，不代表真实模型服务容量，也未与 ZCode 使用相同硬件、协议和数据集做公平吞吐对比。

原始输出见 `temp_dir/subagent-message-pressure*`，资源摘要由 `scripts/benchmark_subagent_messages.py` 生成。
