# 舵机中断后的重复报错验证

实际交互复现：按 `Esc` 后先出现原始取消错误，随后 stream 的取消终态覆盖已记录失败，关闭 stream 时又出现空输出错误。新 stream 必须不继承此前错误。

## 模型与真实代码

| 动作 | 代码边界 |
| --- | --- |
| 收到原始错误 | `crates/wunder-cli/tui/app.rs` 的 `apply_stream_event` 的 `error` 分支 |
| 收到后续取消终态 | 同文件 `turn_terminal` 分支和 `ThreadRegistry::record_terminal` |
| 关闭并结算 | `TuiApp::handle_stream_message` 的 `StreamMessage::Done` 分支 |
| 下一轮重置 | `crates/wunder-cli/tui/thread_registry.rs` 的 `insert_stream` |

模型比较的是本次修复中间版本（已经记录原始失败，但后续取消仍覆盖失败）与最终保留失败的版本，不是仓库最早版本。`Fixed` 只改变 `RecordCancelled` 是否覆盖原始失败，三个不变量在两版配置中保持不变。

## 结果与重放

- 修复前 `NoDuplicateEmptyError` 违反；第一轮反例按图 1 的第 1～3 步到达重复错误，第二轮反例按图 2 的第 1～6 步再次到达同类错误。
- 两条路径均由真实中断交互和定向回归覆盖相同的事件顺序；第二轮完整前缀的“无输出终态”分支仅由模型覆盖，未单独做真实模型重放。
- 修复后 `TypeOK`、`NoDuplicateEmptyError`、`FreshStreamReset` 均通过；模型摘要含 12 个状态、12 条转移，图显示 14 个状态、15 条转移，覆盖视图省略 2 条边，未省略状态。
- `summary.json` 中两版的 `uncited` 均为空。完整 TLC 运行结束后才记录结果，未把部分输出当作通过。
- `a_reported_stream_error_does_not_add_an_empty_output_error_on_done` 用真实 TUI 事件入口重放 `error → turn_terminal(cancelled) → Done`，验证只有一个可见错误；注册表回归检查失败不被同轮终态覆盖且新 stream 重置。

## 重跑

```powershell
$env:JAVA = "<verified-java>"
$env:TLA2TOOLS = "<verified-tla2tools.jar>"
node <statechart-skill>/scripts/statechart.mjs from-spec TuiTerminal.tla --fixed TuiTerminalFixed.tla --map TuiTerminal.map.mjs --expect-broken NoDuplicateEmptyError --out report --cover --png --title "舵机中断后的重复报错"
```

附 `chart.mmd`/`chart.png`、两张 `bug` 图及 Mermaid 源码、`stories.md`、源码和配置；仅保留可移植摘要，不归档含本机目录的原始 TLC 日志。

这是两个 stream、无正文输出条件下的有限抽象。模型没有覆盖网络重连、任意事件交错、整个线程运行时或终端绘制，不能推广为全系统形式化证明。
