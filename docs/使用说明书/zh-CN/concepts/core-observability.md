---
title: 可观测性
summary: 系统如何解释自己：日志、运行时指标、渠道与 WS 事件、监控与压测入口的分工，以及从错误包络出发的排障顺序。
read_when:
  - 出错后要定位：从错误响应追到日志、监控与线程轨迹
  - 需要判断该看监控、渠道运行日志还是压测面板
  - 配置日志目录、保留期或核对性能口径
source_docs:
  - crates/wunder-runtime/src/core/logging.rs
  - crates/wunder-runtime/src/core/runtime_metrics.rs
  - crates/wunder-runtime/src/api/ws_log.rs
  - crates/wunder-runtime/src/channels/service.rs
  - crates/wunder-runtime/src/api/user_channel_logs.rs
  - crates/wunder-runtime/src/api/admin/channel_admin.rs
  - crates/wunder-runtime/src/api/admin/monitor_admin.rs
  - crates/wunder-runtime/src/ops/throughput.rs
  - crates/wunder-runtime/src/ops/performance.rs
  - crates/wunder-runtime/src/api/chat/stress.rs
  - crates/wunder-runtime/src/api/errors.rs
updated_at: 2026-10-13
---

# 可观测性

可观测面分四类：**日志**（发生了什么）、**运行时指标与告警**（现在累不累）、**监控与会话轨迹**（这个线程怎么了）、**性能与压测**（系统能承受多少）。各自有界、各管一段，排障时按顺序串起来。

## 日志

| 项 | 事实 |
| --- | --- |
| 双通道 | 控制台彩色层 + JSONL 文件层同时输出 |
| 文件 | 按天滚动（daily rolling），基名 `server.jsonl` |
| 目录 | `observability.server_log_dir`，默认 `./config/data/logs/server` |
| 级别 | 默认 `info`，`RUST_LOG` 可覆盖，内置第三方噪声降噪 |
| sandbox 模式 | `server_mode=sandbox` 时**不写文件**，只有控制台 |
| 保留期 | `server_log_retention_days` **默认 0 = 永不清理**；设为正整数才按天删除旧文件 |

注意：「默认保留 14 天」是旧文档的错误说法。长期运行的部署要么显式配置保留期，要么自行轮换日志目录。舵机与 bridge 的日志默认级别更严（warn 系）且都不落文件。

## 运行时指标与告警

`/wunder/admin/runtime_metrics` 暴露一份快照，族别固定：

| 族 | 看什么 |
| --- | --- |
| `blocking` | 分类阻塞池（DB/FS/CPU/外部）的调用数、队列等待与超时 |
| `queues` | 有界队列的积压与 busy 拒绝 |
| `long_tasks` | 后台长任务的运行数与告警 |
| `loop_ticks` | 各调度循环的心跳是否停摆 |

告警带阈值与严重度：同一 blocking 池**先判超时类 critical，再判延迟类 warning**——看到 latency warning 之前先确认没有 queue/exec timeout 的 critical。

## WS 与渠道事件日志

- **WS 结构化日志**：`target="wunder.ws"`，字段 `ws_event` 取 `open` / `ready` / `handshake` / `handshake_error` / `message` / `parse_error` / `close`。连接建不上、握手失败、消息解析炸，都在这七个事件里分辨。
- **渠道运行日志**：内存环形缓冲 **300 条 + 20 秒洪泛去重窗**，不落库。查询端点分两路——用户面 `/wunder/channels/runtime_logs[/probe]`，管理面 `/wunder/admin/channels/runtime_logs[/probe]`；主动连通性用 `probe`，长连接异常用 `reconnect`。（旧文档里的 `/wunder/admin/channel/runtime/probe` 不存在。）

## 监控与线程轨迹

`/wunder/admin/monitor` 是治理视图入口：会话列表按状态机 `running / waiting / queued / finished / error / cancelling / cancelled` 标注，可按会话导出 thread-log、翻页 turns、拉快照（`/wunder/admin/monitor/{session_id}/thread-log/export|turns|snapshot|changes`）。舰桥调试面板右侧的日志区已重构为**线程轨迹视图**，追单个线程的执行台账从这里进。

## 三类性能入口，别混用

| 入口 | 用途 | 关键上限 |
| --- | --- | --- |
| 吞吐量（`/wunder/admin/throughput/*` + `/wunder/throughput/ws`） | 模型流式承压：并发 × 输入/输出 token | 并发 ≤ 1024；输入 preset 1024…1,048,576、输出 preset 1024…8192，硬上限输入 16,777,216 / 输出 1,048,576；历史保留 50 条。WS 观看通道靠一次性 ticket 免管理员门 |
| 性能采样（`/wunder/admin/performance/sample`） | 真实环境体检：命令/IO 等采样 | 固定采样用户 `performance_admin`、根目录 `.wunder_perf`、单命令超时 5 秒 |
| 线程渲染压测（`POST /wunder/chat/stress-threads`，**用户面**） | 造大量轮次数据测前端渲染与投影 | 用户轮次 ≤ 2000、模型轮次 ≤ 2000、总条目 ≤ 6,000,000（校验式 `user×(model×5+1)`）；并发追踪 job ≤ 8，生成超时上限 1 小时 |

吞吐与性能属管理面；渲染压测端点刻意放在用户面（供普通环境自测），但单 job 数据量上限极高，属于已知工程注意项。

## 性能评分口径

蜂窝、蜂巢、舰体**三形态各自独立 0–100 加权评分，不可跨形态相加**；对比一律按「同环境、同入口、改前/改后」口径进行，配套基线在 `docs/性能基线/`。换机器、换入口的数字不可互比。

## 排障顺序

1. **拿错误包络**：所有 API 错误统一返回 `{ok:false, error:{code,message,status,hint,trace_id,timestamp}}`，并回写响应头 `x-trace-id` 与 `x-error-code`。先记 trace_id。
2. **查监控**：按会话看状态机与轮次；线程卡住先分清 `waiting`（等人工）与 `queued`（排队/锁）。
3. **对日志**：用 trace_id 在 JSONL 里检索；实时连接问题过滤 `wunder.ws` 事件；渠道不通查运行日志与 probe。
4. **进轨迹**：线程轨迹视图逐轮核对模型调用、工具批次与结果，定位第一处偏离。

## 常见误解

- **「日志默认只留 14 天」**——错。保留期默认 0，永不清理；容量治理要显式配置。
- **「sandbox 也写日志文件」**——不写。
- **「渠道日志能查历史」**——只有内存环形 300 条，进程重启即空；长期审计要接外部日志。
- **「渲染压测是管理员功能」**——`POST /wunder/chat/stress-threads` 在用户面，用普通用户凭据调用。
- **「吞吐、性能、压测是一个面板」**——三者对象与口径不同：网络与模型承压 / 环境采样 / 前端数据量。
- **「跨形态分数能相加」**——不能，评分体系按形态独立加权。

## 相关页

- [性能与可观测运维](/docs/zh-CN/ops/benchmark-and-observability/) — 部署侧操作与验收脚本
- [渠道运行](/docs/zh-CN/ops/channel-runtime/) — 渠道排查细节
- [流式事件参考](/docs/zh-CN/reference/stream-events/) — WS 事件字段
- [舰桥面板指南](/docs/zh-CN/reference/admin-panels/) — 监控/调试面板入口
