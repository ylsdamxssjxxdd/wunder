# 互通空闲预算采样

对应《云端本地互通方案》§13.6 的第一条：隧道空闲时 CPU < 1%、内存增量 < 30 MB。

CPU 与 RSS 是进程级指标，所以采样对象是**回环测量测试自己**：`crates/wunder-runtime/tests/interlink_loopback.rs`
里的 `idle_tunnel_stays_open_without_growth`（`#[ignore]`）在同进程内拉起真实舰体 router 与真实本地引擎。
两个引擎各有自己的巡检定时器，直接测整进程会把它们算到隧道头上，所以这个测试跑**两段**：

1. `baseline`：同进程、隧道未起，空闲 N 秒；
2. `tunnel`：起一条真实隧道并完成握手后，再空闲 N 秒（打印 `INTERLINK_TUNNEL_UP` 作为分界）。

每段都不断言性能，只在段尾断言链路仍在且每个有界结构回空；进程开销由本目录脚本从外部按段采样，**预算只作用于增量**（tunnel 段 CPU 速率 − baseline 段 CPU 速率）。

## 运行

```powershell
powershell -File scripts/interlink-bench/measure-idle.ps1 -IdleSeconds 60 -Release `
  -OutCsv C:\path\to\temp\interlink-idle.csv -OutJson C:\path\to\temp\interlink-idle.json
```

- 脚本先跑 `cargo test -p wunder-runtime --test interlink_loopback --features sqlite-storage -- --ignored --nocapture --test-threads=1 idle_tunnel`。
  `--test-threads=1` 是必须的：隧道会话在进程全局的 `cloud::shared()` 里，与主链路测试并跑会互相抢会话。
- `-IdleSeconds` 是**每段**秒数，整轮约 `2 * IdleSeconds`；`-Release` 用于出文档数字（预算是产品预算，debug 每个 tick 贵数倍）；`-CompileBudgetSec` 要按冷编时长放大。
- 从测试输出里抓 `INTERLINK_IDLE_PID=<pid> BASELINE_SECONDS=<n> TUNNEL_SECONDS=<m>` 标记，再按 `-IntervalMs` 采样该 pid 的 `WorkingSet64`/`PrivateMemorySize64`/`TotalProcessorTime`，并用 `INTERLINK_TUNNEL_UP` 给每帧打段标签。
- 判定：`tunnel_net_cpu_percent <= -MaxCpuPercent`（默认 1.0）且 `tunnel_net_delta_mb <= -MaxRssDeltaMb`（默认 30.0），且测试本身通过；超预算退出码 1。两段任一样本不足即报错退出，不允许"只测到一段"的假绿。
- 产物：CSV 为逐帧采样（含 `phase` 列），JSON 为分段汇总（`baseline_cpu_percent` / `tunnel_cpu_percent` / `tunnel_net_cpu_percent` / `baseline_delta_mb` / `tunnel_delta_mb` / `pass`）。
- `-OutCsv` / `-OutJson` 必须是 Windows 绝对路径：Git Bash 里 `$TEMP` 会展开成 `/tmp`，PowerShell 会把它当成当前盘的相对路径。

结果按《性能基线》的写法落 `docs/性能基线/`，写实测数字与机器信息，不写目标值。

## 同目录的其它预算项

`§13.6` 其余条目由 `interlink_budget` 测试直接钉住，不需要外部采样：

- 台账/审计在 1000 行规模下 P95 < 500ms（并断言 `(user_id, created_at)` 与 `created_at` 两个索引存在）。
- 影子投影在 500 条目上限处截断且带 truncated 标记，本地全量计算 < 500ms。
- 远程订阅每节点上限 8，单条上行帧对 8 个读取者聚合分发（`watch_list` 仍是一大对）。

```bash
cargo test -p wunder-runtime --test interlink_budget --features sqlite-storage
```
