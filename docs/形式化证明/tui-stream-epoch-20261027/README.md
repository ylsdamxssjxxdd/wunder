# 舵机 TUI 流事件轮次边界验证

本验证针对多轮交互后输出被打乱或缺失的状态 bug：传输事件编号由每次 stream 的生命周期生成，旧投影却把去重集合保留在线程生命周期，下一轮从编号 1 开始时会误判为重复并丢弃事件。

## 结果

- 修复前模型：`TypeOK` 通过；`EveryArrivedEventRenders` 按预期失败，最短路径为“第一轮收到事件 → 开始第二轮 → 第二轮收到同编号事件”。
- 修复后模型：`TypeOK` 与 `EveryArrivedEventRenders` 均通过。
- TLC 检查了 72 个模型状态、116 条模型转移；图表覆盖 3 个状态、4 条转移，无遗漏。
- `summary.json` 的 `uncited` 为空，说明模型动作均绑定到生产代码位置。

## 真实代码证据

- `ThreadRegistry::insert_stream` 在建立新 stream 时清空传输事件去重集合、活动事件号和上一轮终态；durable `change_seq` 游标不清空。
- `TuiApp::apply_stream_event` 仍通过 `mark_event_applied` 做单轮内去重，避免同一事件重复投影。
- `crates/wunder-cli/tui/app/snapshot_tests.rs` 增加轮次重置回归；`scripts/experience-bench/measure-tui.py` 用真实 Windows ConPTY、隔离 mock 流模型、多轮输入与窗口缩放验收。

## 重跑

```powershell
Push-Location docs/形式化证明/tui-stream-epoch-20261027
$env:JAVA = "<verified-java>"
$env:TLA2TOOLS = "<verified-tla2tools.jar>"
node <statechart-skill>/scripts/statechart.mjs from-spec TuiStreamEpoch.tla --fixed TuiStreamEpochFixed.tla --map TuiStreamEpoch.map.mjs --expect-broken EveryArrivedEventRenders --out report --cover --png
Pop-Location
```

模型是有限抽象，只覆盖传输事件编号轮次边界，不等价于整个运行时、provider 或终端的定理证明。

归档内容只保留可移植的模型源码、配置、摘要、图表和反例故事；TLC 原始日志未归档，避免把本机临时目录写入仓库。
