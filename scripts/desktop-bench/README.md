# desktop-bench

蛹的外部自动化体验测试与性能测量工具。默认使用独立后端探针，按 [蛹体验测试方案](../../docs/蛹体验测试方案.md) 持续测试和优化。

保留旧的桌面进程启动、内存和包体积比测作为可选补充。测试输出 JSON/CSV 原始数据及摘要。

## 脚本

### measure-experience.py — 蛹统一后端体验测试

后续测试和优化以 [蛹体验测试方案](../../docs/蛹体验测试方案.md) 为准。默认外部运行独立 Release 探针，包含 100×100 日志打开、提交、流式/慢消费、停止后继续、队列控制、线程切换、后台执行、目录分页、启动/正常重启恢复和持续运行。不需要最终蛹程序或前端。固定配置可重复执行、比较基线，正确性失败/超时/退化返回非零码。

```powershell
python scripts/desktop-bench/measure-experience.py --profile smoke --output target/desktop-experience/smoke-001
python scripts/desktop-bench/measure-experience.py --profile daily --output target/desktop-experience/daily-001
python scripts/desktop-bench/measure-experience.py --profile daily --baseline target/desktop-experience/daily-001/report.json --output target/desktop-experience/daily-002
```

`--profile soak` 默认运行 2 小时；`--cases` 可定向复测。强杀恢复、工具阻塞取消和双线程同时输出在方案中明确列为下一批扩充，当前不会把后端结果当作前端渲染通过证据。下面的旧桌面进程黑盒比测工具保留为可选补充。

### measure-chat-open.py — 聊天打开后端基准

独立测试程序直接调用蛹聊天页使用的原生后端库，不依赖最终蛹 EXE、Slint 或桌面环境。自动准备 100×100 固定夹具、重复测量首次打开/缓存返回/存储读模型/历史分页，采集资源计数并比较基线，失败或性能退化返回非零码。使用方法与性能分析见 [chat-open.md](chat-open.md)。

```powershell
python scripts/desktop-bench/measure-chat-open.py --output target/desktop-chat-open/baseline
```

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
- 聊天打开的后端耗时使用上述独立基准；页面可交互、渲染、流式与重连的端到端耗时另测，不能用后端接口时间代替用户实际看到内容的时间。
