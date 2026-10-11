# 2026-10-11 runtime 互通隧道空闲预算

对应《云端本地互通方案》§13.6 第一条：隧道空闲时 CPU < 1%、内存增量 < 30 MB。本报告只记实测数字与采样方法，不写目标值以外的推断。

## 目标

- 被测链路：互通隧道客户端引擎的一条真实空闲隧道（握手完成、心跳/存在通告/影子合并窗口在跑，无命令、无远程订阅）。
- 风险点：隧道常驻定时器若按 tick 做重活（全量会话投影、整份配置克隆、工作区遍历），空闲成本会落在本地引擎主链上，蜂窝/舵机侧表现为「没在用也发热」。

## 环境

- 代码状态：`main`（互通方案 P0–P4 落地后，未提交）。
- 机器：AMD Ryzen AI Max+ 395（32 逻辑处理器），Windows 10.0.26200 x64。
- 工具链：cargo 1.95.0 / rustc 1.95.0，`--release`。
- 数据库：进程私有 SQLite（`tempdir` 内独立库，不碰 `config/wunder.yaml` 与共享开发库）。
- 运行方式：`cargo test -p wunder-runtime --test interlink_loopback --features sqlite-storage --release -- --ignored --nocapture --test-threads=1 idle_tunnel`，由 `scripts/interlink-bench/measure-idle.ps1 -IdleSeconds 60 -IntervalMs 500 -Release` 从外部采样。
- 关键配置：`interlink.heartbeat_s`、`shadow_interval_s`、`shadow` 限额全部取默认值。

## 采样方法

同进程里有**两套引擎**（服务端 router + 本地形态），它们各自的巡检定时器会盖过隧道的成本，所以单段测量得到的不是隧道的账。改成同进程两段：

1. `baseline`：进程已建好两套引擎、隧道**未起**，空闲 60s；
2. `tunnel`：起一条真实隧道并完成握手后，再空闲 60s（测试打印 `INTERLINK_TUNNEL_UP` 作为分界，脚本给每帧打段标签，不用推算时间点）。

每段独立算 `Δcpu_ms / Δwall_s`（**单核百分比**，非全机百分比）与该段工作集的 min→peak。预算判定只作用在增量上：`tunnel.cpu_pct - baseline.cpu_pct <= 1.0`，`tunnel 段工作集增长 <= 30 MB`。`--test-threads=1` 是硬要求：隧道会话在进程全局 `cloud::shared()` 里，与主链路测试并跑会互相抢会话。

## 对比结果

| 指标 | baseline（两引擎，无隧道） | tunnel（+1 条空闲隧道） | 隧道增量 | 预算 | 结论 |
|---|---|---|---|---|---|
| CPU（单核 %） | 0.651 | 0.764 | **+0.114** | < 1.0 | 通过 |
| 工作集（MB） | 15.98 → 16.70（+0.72） | 21.43 → 21.61（**+0.18**） | +0.18 | < 30 | 通过 |
| 采样窗口（s） | 60.04（120 帧） | 59.27（约 119 帧） | — | — | — |
| 测试自身 | — | `test result: ok`，exit 0 | — | 必须通过 | 通过 |

原始资料：`assets/2026-10-11-runtime-interlink-idle-tunnel-samples.csv`（逐帧，含 `phase` 列）、`assets/2026-10-11-runtime-interlink-idle-tunnel-summary.json`（汇总）。

两点要如实记下：

- 两段之间工作集从 16.7 跳到 21.4 MB（约 **+4.7 MB**），这是**建链一次性开销**（TLS/WS 缓冲、影子投影缓冲、握手期分配），不是空闲窗口内的增长；空闲窗口内的增长是 0.18 MB。若把它算作「内存增量」，需要另立一条建链预算，而不是塞进 §13.6 的空闲条里。
- debug profile 下同一 harness 的**整进程** CPU 为 1.77–1.90%（单段法，含两引擎定时器，且比 release 贵数倍）。这不构成「隧道超预算」的结论，只说明两件事：预算必须在 release 上量，以及必须按段取增量。文档与门禁都以 release 双段数字为准。

## 同轮改动与对照

测量口径建立后回头修了空闲 tick 上的两处无谓开销（都在 `services/interlink/client/mod.rs` 的 2s 合并窗口路径）：

- `active_threads()` 原来调 `monitor.list_sessions(true)`，那会为每个活跃会话构建整份 JSON 摘要并重算 LLM 速率，而调用方只取 `.len()`；改为新增的 `MonitorOps::count_active_sessions()`（只数不改）。
- `pump_shadow_signals()` 原来为了拿工作区 scope 先 `shadow_sources().await` 建整个 `ShadowSources`，而 `tree_limits()` 会 `config_store.get().await` **整份克隆 Config**；`workspace_scope()` 改为直接接收身份字段（`local_user_id` / `workspace_id`），脏区为空时不再克隆配置。

这两处在本次 release 数字里量不出差别（隧道增量本来就只剩 0.114 个点），修它们的理由是「有界与不克隆大对象」本身，不是为了解释某个数字。

## §13.6 其余条目

不进本报告，由 `tests/interlink_budget.rs` 直接钉住（`cargo test -p wunder-runtime --test interlink_budget --features sqlite-storage`，本机 debug 约 20s 全绿）：

- 索引存在：`idx_interlink_commands_user_created`、`idx_interlink_audit_created`。
- 1000 行台账 + 1000 行审计下，过滤分页查询各 25 次采样，P95 < 500ms。
- 影子投影 12×50=600 个文件时恰截断在 500 条并带 `truncated`，本地全量计算 < 500ms，且载荷不含绝对根路径。
- 远程订阅每节点上限 8：第 9 个直接拒绝，8 个读取者共用一条上游 `thread.watch`（`watch_list` 只有一个 (node, thread) 对，计数 8），最后一个退订才撤销上游兴趣。

「影子全量计算不阻塞 Slint 帧（≥55fps）」这一条本报告**没有**数字：投影跑在引擎的异步任务里、不在 UI 线程，结构上不占帧，但帧率本身要在真蜂窝上量才算数。

## 复跑

```powershell
powershell -File scripts/interlink-bench/measure-idle.ps1 -IdleSeconds 60 -IntervalMs 500 -Release `
  -OutCsv C:\path\to\temp\interlink-idle-release.csv -OutJson C:\path\to\temp\interlink-idle-release.json
```

超预算时脚本退出码 1 并打印 `BUDGET EXCEEDED`；任一段采样不足会直接报错退出，不允许「只测到一段」的假绿。
