# wunder benchmark workflow

本仓库使用 `scripts/benchmark.ps1` 作为统一性能基准入口。

## 运行 baseline

```powershell
powershell -ExecutionPolicy Bypass -File scripts\benchmark.ps1 -Label baseline
```

默认会收口当前最稳的后端性能链路：

- `backend_sim` 标准回归
- 可选 `runtime_boundary_stress.py`

输出目录默认写入 `target\bench\<timestamp>-<label>\`：

- `benchmark.json`
- `benchmark.csv`
- `benchmark.md`
- `logs/`

## 对比优化

使用上一轮 `benchmark.json` 作为 baseline：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\benchmark.ps1 `
  -Label after-change `
  -Baseline target\bench\YYYYMMDD-HHMMSS-baseline\benchmark.json
```

`benchmark.json` 会保留每项结果的基线值、绝对变化和百分比变化。默认回退门槛：

- 延迟类：劣化超过 10% 且绝对增加超过 50ms
- 吞吐类：下降超过 5%
- 比例类：变化超过 0.1% 时记录为回退候选

## 扩展方式

- 新增后端性能项，优先接入 `scripts/run_backend_sim_workflow.py`
- 新增运行时边界项，优先接入 `scripts/runtime_boundary_stress.py`
- 模型能力质量仍使用 `WunderBench`，不要混入这个基准入口

## 备注

- 这个入口只负责“可比较的性能基线”
- 真实 IO 必须保留真实挂载语义
- 任何新性能项都应先有 baseline，再谈优化
