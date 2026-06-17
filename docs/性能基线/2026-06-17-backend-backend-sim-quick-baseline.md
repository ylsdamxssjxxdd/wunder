# 2026-06-17 backend backend_sim quick 基线

## 目标

- 建立第一波可比后端 benchmark 实测数据。
- 验证统一入口 `scripts/benchmark.ps1` 能生成 `benchmark.json/csv/md`，后续性能优化可使用同入口做前后对比。

## 环境

- 运行方式：本机 Windows PowerShell 5.1，release 构建。
- Rust：`rustc 1.94.1 (e408947bf 2026-03-25)`。
- Cargo：`cargo 1.94.1 (29ea6fb6a 2026-03-24)`。
- Python：`Python 3.12.10`。
- Git head：`621f99cb`。

## 采样方法

- 命令：`powershell -ExecutionPolicy Bypass -File scripts\benchmark.ps1 -Quick -Label baseline`
- 入口：`scripts/benchmark.ps1`
- 场景：`backend_sim` quick 三场景。
- 重复次数：本次为第一波单次基线，后续接近阈值时需要重复 3 次取中位数。

## 基线结果

| 场景 | 成功率 | p95 | p99 | 首事件 p95 | 吞吐 | final missing |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| stream_high_concurrency | 100.00% | 667.13ms | 743.66ms | 433.18ms | 15.87 rps | 0.00% |
| stream_shared_session | 80.00% | 1136.97ms | 1196.09ms | 1101.29ms | 8.95 rps | 20.00% |
| run_non_stream | 100.00% | 556.09ms | 584.95ms | n/a | 14.91 rps | 0.00% |

## 结论

- 判定：通过。
- 说明：这是第一波 quick 基线，主要用于确认 benchmark 工作流与后续对比格式。`stream_shared_session` 的 `USER_BUSY` 属于共享会话锁竞争场景的预期压力信号，本次在 quick 阈值内。
- 后续动作：优化后使用本次 `benchmark.json` 作为 `-Baseline` 输入执行同入口对比。

## 原始资料

- 标准报告：`target\bench\20260617-144547-baseline\benchmark.md`
- 结构化数据：`target\bench\20260617-144547-baseline\benchmark.json`
- CSV 数据：`target\bench\20260617-144547-baseline\benchmark.csv`
- 后端仿真摘要：`target\bench\20260617-144547-baseline\backend_sim\summary.baseline.20260617T064601Z.json`
