# 思考流一致性验证（桌面实时投影）

## 现象

- 思考条目偶发失去流式：整段直接贴出，或中途停在一句话中间；
- 后出现的工具条目已正常结算，**排在它前面的思考条目仍在转圈**；
- 当线程里有等待自动发送的消息（排队/插话）时更容易出现。

## 抽象

把 reducer 的思考生命周期抽象成三个对象：

| 模型 | 代码 |
| --- | --- |
| `rows`（按创建顺序追加的行） | `Timeline.model`，行由 `push` 追加 |
| `slot`（思考槽，0 表示空） | `Timeline.thinking: Option<Thinkingindex}>` |
| `clock`（当前模型轮次） | `TurnOutput.round`，由 `llm_request`/`llm_output`/delta 推进 |

动作与代码的对应：

| 动作 | 代码位置 |
| --- | --- |
| `Reason(r)` | `timeline.append_reasoning_item` + `apply_event` 的 `round < active.round` 守卫 |
| `ToolCall(t)` | `timeline.upsert_tool` 的 `pending` 分支 |
| `ToolResult(t)` | `timeline.upsert_tool` 的非 `pending` 分支 |
| `Advance` | `apply_event` 推进 `active.round` |
| `Finish` | `Timeline.finish` / `interrupt_running` |

三条不变式（读者可见的正确性）：

1. `NoStaleRunning`：只有最新一行可以是"运行中"。
2. `NoRunningBehindAction`：一条思考行后面如果已经出现了工具行，它就不能还在转圈。
3. `TextKept`：已产生的思考文本必须都已显示（丢帧即永久缺字）。

## 缺陷与反例

`models/ThoughtStream.tla` 保留当前实现的三个真实分支，TLC 均给出反例：

| 缺陷 | 代码 | 反例轨迹 |
| --- | --- | --- |
| 工具帧早退 | `upsert_tool` 命中已存在 id 时 `return`，而 `close_thinking` 在分支之外 | `Reason(r1)` → `ToolResult(t1)` → `ToolCall(t1)`（早退）→ 思考仍 running，工具行已在其后 |
| 迟到思考帧被丢弃 | `apply_event` 中 `round < active.round` 直接 `return Ok(false)` | `Advance` → `Reason(旧轮)` → `shown < seen`，文本永久缺失 |
| 结果先到不结算 | `upsert_tool` 只有 `pending` 才关闭思考 | `Reason(r1)` → `ToolResult(t1)` → 违反 `NoStaleRunning` |

最短反例见 `反例轨迹.svg`。

## 修复与证明

`models/ThoughtStreamFixed.tla` 对应修复后的实现：

- 任何一个工具帧到达（新条目、结果先到、迟到重复的 call）都先结算思考行；
- 迟到思考帧不再丢弃，而是写进它自己那一轮的已结算行（`append_settled_reasoning`），不重开思考；
- 重新开始同一轮（等待消息重发、插话复用行、快照接管）时先结算上一行（`begin_turn_in` / `forget_turn` 用 `close_thinking` 代替直接清槽）。

TLC 穷举结果（`Rounds={1,2}`、`Tools={1,2}`、`MaxSeen=3`）：

| 模型 | 不变式 | 结果 |
| --- | --- | --- |
| ThoughtStream | `NoRunningBehindAction` | 反例（13 distinct states） |
| ThoughtStream | `NoStaleRunning` | 反例 |
| ThoughtStreamFixed | `TypeOK` | 通过（3024 distinct states） |
| ThoughtStreamFixed | `NoRunningBehindAction` | 通过 |
| ThoughtStream | `TextKept` | 反例 |
| ThoughtStreamFixed | `NoReasonAfterCalled` | 通过 |
| ThoughtStreamFixed | `TextKept` | 通过 |

## 代码映射（修复点）

| 代码 | 变化 |
| --- | --- |
| `frontend/src/timeline.rs: upsert_tool` | `existing.is_none() \|\| pending` 时先 `include_active_body_in_process` + `close_thinking`，早退只跳过行内容更新 |
| `frontend/src/timeline.rs: append_reasoning_item` | 同 id 行已存在且不是当前思考行时，`append_settled_reasoning` 补写文本，不再新建重复行 |
| `frontend/src/timeline.rs: begin_turn_in` / `forget_turn` | 用 `close_thinking(false)` 取代 `thinking = None`，重开一轮时先结算上一行 |
| `frontend/src/native_chat.rs` | 迟到帧的丢弃规则保持原样（既有测试守护"迟到帧不重开更早的轮次"） |

## 复验

```bash
python3 scripts/check_thought_stream_formal.py --keep
```

脚本要求：缺陷模型必须报指定反例，修复模型必须枚举完整个状态空间且无错；任何偏差以非零码退出。TLC 状态与日志写入仓库外（默认临时目录，`--out` 可指定）。
