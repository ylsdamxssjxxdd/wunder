# desktop-bench

桌面端性能黑盒测量工具（蜂窝 vs dsh 桌面端比测用，见 `docs/方案/智能体性能测试对比方案.md` 第 8 节 SOP）。

同一套脚本测两个应用，口径天然一致。所有脚本输出 JSON/CSV 原始数据 + 摘要。

## 脚本

### measure-startup.ps1 — T1 启动耗时

进程启动 → 主窗口句柄出现，轮询粒度约 10ms。按进程名聚合，覆盖 Electron 多进程树。每个样本跑完自动结束整个进程树。

```powershell
powershell -ExecutionPolicy Bypass -File scripts\desktop-bench\measure-startup.ps1 `
  -ExePath <应用exe> -Runs 5 -WarmupRuns 1 -OutJson out\startup-<app>.json
```

### measure-memory.ps1 — M1–M4 内存端点

对指定进程名的全部实例求和采样（Working Set + Private Bytes），输出逐样本 CSV 与端点摘要（起始/稳态/峰值/进程数）。配合场景脚本或手动触发动作使用。

```powershell
powershell -ExecutionPolicy Bypass -File scripts\desktop-bench\measure-memory.ps1 `
  -ProcessName <进程名> -DurationSec 600 -IntervalMs 1000 `
  -Label m1-idle -OutCsv out\m1-<app>.csv -OutJson out\m1-<app>.json
```

### measure-package.ps1 — P1–P3/P5 包体积

传安装包文件则测安装包大小；传目录则测总大小、文件数、exe/dll/node/asar 计数与最大 10 个文件。

```powershell
powershell -ExecutionPolicy Bypass -File scripts\desktop-bench\measure-package.ps1 `
  -Target <安装包或安装目录> -Label pkg-<app> -OutJson out\pkg-<app>.json
```

### summarize.py — 汇总并排表

把多份 JSON 报告按 startup / memory / package 分组合成 markdown 对比表。

```powershell
python scripts\desktop-bench\summarize.py out\startup-wunder.json out\startup-dsh.json --out out\summary.md
```

## 比测最小流程（第一批黑盒场景）

1. 两边装好最终产物，记录版本与构建参数。
2. 各跑 `measure-startup`（1 预热 + 5 样本）。
3. 启动后静置 10 分钟，各跑 `measure-memory -DurationSec 600`。
4. 各跑 `measure-package`（安装包 + 安装目录各一次）。
5. `summarize.py` 合表，归档到 `docs/性能基线/`。

## 注意

- 测量期间关闭高负载后台程序；报告必须附环境清单与原始样本。
- T2–T9（可交互、线程打开、长会话渲染、流式、重连）需要应用内埋点（wunder 侧计划加 `--bench-scenario` 参数），属第二批实施。
