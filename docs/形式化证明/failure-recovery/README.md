# 模型与工具故障恢复验证

日期：2026-10-28。对象是蛹、舵机与舰体复用的 `wunder-runtime`，不是只检查某个界面。

## 结论与修复

旧链路不能称为完全稳定。首轮复现并修复以下四类缺口；同日续验的协议修复见下一节：

1. HTTP 状态和供应方错误码被格式化成字符串后丢失语义，永久拒绝会重复请求，正文中的 `timeout`/`429` 还可能误触发暂时故障策略。新增 `LlmHttpError`，沿 `anyhow` 错误链优先识别状态；鉴权、权限、参数、额度不足不原样重试，返回 `LLM_REQUEST_REJECTED` 和 `fix_request`，而非内部错误。
2. 流式 HTTP 暂时故障直接切非流式，绕过引擎退避，单次故障最多引起双倍请求。现在暂时故障统一进入可取消的引擎退避；只有明确不支持流式的能力错误允许兼容降级，`stream_options` 也仅在响应明确提及时移除。
3. 带正文、工具参数或 usage 的 SSE 在没有完成标记、没有结束原因时 EOF，旧代码仍返回成功。现在必须收到完成标记或 `finish_reason`；不完整工具提议不会进入工具执行链。仍兼容尾事件无换行但有结束原因、`[DONE]`、Responses 完成和 Anthropic 停止。
4. 工具/命令超时不代表没有执行。旧建议标为 `retryable=true` 并引导直接重试，可能重复写入。共用超时元信息改为 `execution_outcome=unknown`、`retryable=false`、无自动重试时间；模型侧精简观察也保留未知结果与核对提示。此举不是持久幂等或强制禁止模型再次调用工具。

主要代码：`src/services/llm/failure.rs`、`src/services/llm.rs`、`src/orchestrator/llm.rs`、`src/orchestrator/error.rs`、`src/services/tools/tool_error.rs`、`src/services/tools/command_tool.rs`、`src/orchestrator/execute_support.rs`、`src/orchestrator/tool_result_payload.rs`，均位于 `crates/wunder-runtime/`。

## 同日续验：协议错误与分块边界

继续使用真实 `LlmClient` HTTP 入口，先运行新增回归再修改实现，8 项中 4 通过、4 失败，实际复现：HTTP 200 的错误封装被接受、正常 `error:null` SSE 被拒绝、跨字节中文/emoji 损坏、无效编码被后续 `[DONE]` 掩盖。真实引擎入口另复现永久拒绝误报 `CONTEXT_WINDOW_EXCEEDED`。

- 新增 `LlmPayloadError`：非流式正文与 SSE 共用错误识别，覆盖嵌套 `response.error`、`response.failed`、`status=failed` 及顶层 `type=error`/`code`；非空错误先于正文或工具提议被拒绝，空错误字段不误伤正常完成。
- 已识别供应方错误码优先于消息文字；永久拒绝、暂时错误、上下文错误分别处理。兼容仅有 `invalid_request_error` 且正文说明上下文超限的供应方，不把所有参数错误都当作上下文故障；未知码仍走既有文本兜底，未宣称穷尽。
- 外层压缩恢复仅在明确 `CONTEXT_WINDOW_EXCEEDED` 或旧式 `INTERNAL_ERROR` 文本匹配时进入，不能把已分类 `LLM_UNAVAILABLE` 再按正文改判；实际引擎回归检查六次耗尽、原消息不变、线程结清及下一轮恢复。
- 新增增量 UTF-8 解码器，跨包只暂存最多 3 个未完成字节；中文、emoji、思考与工具 JSON 参数保真，不按整个网络包做有损替换。该 3 字节界限只针对解码残片，不是整个 SSE/历史消息内存上限。
- 无效 UTF-8、未完成码点及损坏 JSON 事件返回错误，不能因后继 `[DONE]` 变成成功；进入原有可取消、有界退避。仍保留既有空成功响应的轮次补救行为、完成标记及输出限额的 `response.incomplete` 语义。

新增模型、五条编号反例和入口对照见 [ProviderProtocol 总图](./ProviderProtocol/chart.png)、[Mermaid](./ProviderProtocol/chart.mmd)、[编号路径](./ProviderProtocol/stories.md)、[验收记录](./验收记录.md)。实现位于 `services/llm/failure.rs`、`services/llm/stream_decode.rs`、`services/llm.rs`、`orchestrator/llm.rs`、`orchestrator/execute_support.rs`，均属共享引擎。

## 有界契约

- 暂时故障普通请求最多 6 次，退避依次为 1.2、3、6、12、20 秒；这是一次引擎模型调用的策略，不是整个用户轮次或所有供应方内部兼容协商的总请求上限。
- 永久拒绝与上下文溢出不重复发送原请求；上下文溢出交给外层既有压缩恢复，而不是伪装为网络故障。
- 模型请求重试使用相同消息，不重写冻结的 system prompt，不重放已经执行并配对观察的工具。
- 取消等待分支被选择后不再重新派发；失败/取消结清后释放本代资源，后续用户轮次可以继续。
- 超时后的外部副作用未知，先核对，不宣称恰好一次。

## 形式化模型

权威模型在 `models/`，修复前后使用相同不变量，仅 `Fixed` 常量不同。实际检查结果、编号路径与图见 `FailureRecovery/`、`ToolTimeout/`；脚本逐份核验 TLC 完成标志，不把 Java 错误或部分输出当成功。

- 故障恢复：[总图 PNG](./FailureRecovery/chart.png)、[Mermaid 源](./FailureRecovery/chart.mmd)、[四条编号路径](./FailureRecovery/stories.md)、[结果摘要](./FailureRecovery/summary.json)。
- 超时副作用：[总图 PNG](./ToolTimeout/chart.png)、[Mermaid 源](./ToolTimeout/chart.mmd)、[两条编号路径](./ToolTimeout/stories.md)、[结果摘要](./ToolTimeout/summary.json)。
- [真实入口验收与旧代码对照](./验收记录.md)。各编号路径同目录均有对应 `bug-N.png` 与 `bug-N.mmd`。

- `FailureRecovery`：四种故障场景、单执行体、单工具、最多 6 次引擎尝试；检查永久拒绝不重试、无隐藏暂时故障降级、不接受截断流、工具不因模型重试而再执行、资源结清与已观察取消闸。`EventuallyReleased` 只在弱公平 `WF_vars(Next)` 且供应方调用/超时和持久化能返回的抽象前提下成立，不是现实毫秒级完成保证。
- `ToolTimeout`：单外部命令、最多两次副作用，检查未知结果不得被标成安全重试，以及遵循建议的调用者不会未经核对重复副作用。外部命令写入和调用者遵循建议是环境动作，不是代码自动重试；本模型不能约束不遵循建议的模型。
- `ProviderProtocol`：五种协议场景、单响应、读取/验证/分类/返回四个抽象观察点；原模型 20 状态、15 转移，五条预期规则违反，修复后保留同组不变量通过。分块解码抽象为“字符保真/损坏”，不是对任意字节串的参数化正确性证明，也不覆盖所有 HTTP 状态与传输组合；解码器真实测试补充逐字节及每个二分边界。模型没有时间活性承诺，外层暂时错误耗尽仅有真实入口回归。
- `FailureRecovery` 的取消变量表示 `select!` 取消分支已经胜出，**不是**停止 API 已受理；响应就绪与停止同时发生、取消闸跨边界的更完整竞态仍以 FV-04 为后续范围。
- `SettledResourcesReleased` 仅覆盖 `released` 观察点；持久化失败、进程崩溃、多执行者与真实 OS 锁不包含在模型中。

## 真实回归与反例映射

回归调用实际 `LlmClient`、`orchestrator.run`、SQLite 及本地命令入口，供应方用本地 HTTP 故障注入，不消耗外部模型额度。

| 模型编号 | 路径与真实回归 | 修复前观察 |
| --- | --- | --- |
| FailureRecovery 1 | 请求 → 暂时故障 → 未退避非流式；`transient_api_failure_recovers_with_unchanged_system_prompt` | 后继请求 `stream` 从 true 变 false |
| FailureRecovery 2 | 请求 → 不完整工具流 → 当作成功；`truncated_tool_stream_is_retried_without_publishing_a_tool_result` | 后继消息被追加空输出恢复提示；修复后同消息重试且不生成目标文件 |
| FailureRecovery 3 | 请求 → 鉴权拒绝 → 重试原请求；`permanent_api_failure_settles_and_next_turn_reuses_the_thread` | 返回 INTERNAL_ERROR 而非明确拒绝；状态分类与直接入口共同覆盖该故事 |
| FailureRecovery 4 | 工具失败观察 → 模型暂时故障 → 未退避非流式；`tool_failure_is_paired_once_and_survives_followup_api_retry` | 流式后继被改为非流式，工具结果配对仍保留 |
| ToolTimeout 1–2 | 命令先写入 → 等待超时 → 可重试建议；`local_command_timeout_preserves_side_effect_and_requires_verification` | 超时已经产生真实写入，却仍标为可重试；模型中的第二次副作用是环境按建议重试，真实测试没有盲目重放命令 |

前四条是在仓库外副本还原本次修改前的两个 LLM 模块后执行同一回归，均按预期失败；未回滚工作树中的其他开发者修改。超时反例另在旧命令路径副本检查。完整原始日志仅在外部 scratch，不能直接提交。

已执行结果：SQLite runtime 一次完整运行 2181 项通过、4 项跳过；随后重跑为 2180 通过、1 项既有子智能体调用计数断言失败（预期 3，实际 4）、4 项跳过；补永久拒绝外层围栏后的全量为 2181 通过、同一既有子智能体用例状态断言失败（预期 pending，实际 running）、4 项跳过。本次故障恢复用例全部通过。最终复验结果见本主题 `验收记录.md`，不把偶发失败隐去。两个模型的修复后规则全部通过，`FailureRecovery` 另通过上述有限前提下的活性性质。归档图均已逐张检查，overview 只用于总览，编号反例图用于逐步审查。`summary.json` 的 `uncited` 路径仅做机器路径脱敏，不改状态/边计数或规则结果。

其他覆盖：HTTP 408/409/425/429/500/502/503/504/529 无隐藏重试，400/401/403/404/422 不被正文误导，额度不足，部分正文/工具/usage EOF，正常终止标记，退避期间停止、六次耗尽后下一轮恢复、system prompt 和工具观察稳定。

## 未覆盖与不能承诺

- 没有证明整个系统永远无错误，也没有勾完 FV-04/FV-16。
- 暂时错误的 `Retry-After`、退避抖动、统一用户轮次墙钟预算仍未实现；管理员模型超时豁免与显式无超时工具配置仍可能无限等待，除非主动取消。
- 流中已展示的部分 token 在重试时的跨端撤回/替换、断流消耗的精确计费不在本次证明中；UTF-8 跨包已补实现及实际入口验证，但不宣称跨端投影因此完整。
- 已补常见 HTTP 2xx/SSE 错误封装和已知错误码；所有供应方 SSE 变体、未知码语义、Responses `incomplete` 的全原因映射、云端鉴权刷新/队列策略仍未穷尽。
- 不保证外部工具/远端命令在超时、失联、进程崩溃后恰好执行一次；需要持久幂等、远端确认和结果核对能力，不能靠重试建议代替。
- 本次在当前 Windows 开发环境和 SQLite 上实测；未实测 Win7 x86、Ubuntu 18.04、PostgreSQL 实库、真实供应方故障及所有 UI 端到端交互。

## 复验

设置已核验 Java/TLC，TLC 1.8.0 SHA256：`78f7e2c6bf9a8b65eda665a23e94431255587e0b43a834a4e2af1723562d9b4a`。Java 使用 Zulu 21.0.4；statechart 来自本地技能。以下路径均为占位符。

```powershell
python scripts/check_failure_recovery_formal.py --statechart <statechart-skill>/scripts/statechart.mjs --out <external-scratch>/failure-recovery
cargo test -p wunder-runtime --features sqlite-storage --lib failure -- --test-threads=4
cargo test -p wunder-runtime --features sqlite-storage --lib services::llm -- --test-threads=4
cargo test -p wunder-runtime --features sqlite-storage --lib -- --test-threads=4
cargo test -p wunder-runtime --features sqlite-storage --test context_overflow_recovery_regression
python -m unittest discover -s tests -p test_failure_recovery_formal.py -v
python -m unittest discover -s tests -p test_formal_proof_layout.py -v
```

`tests/test_failure_recovery_formal.py` 用替身检查资料复制和输出围栏，不运行 TLC，不能算形式化证明。
