---
title: 子智能体控制
summary: `子智能体控制`（别名 `subagent_control`）派生、跟踪、引导与回收临时子智能体工作单元。
read_when:
  - 需要把一块独立工作派生给子智能体，或批量并发派生
  - 需要等待、中断、关闭或恢复子智能体，或读它的进展与历史
source_docs:
  - crates/wunder-runtime/src/services/tools/subagent_control.rs
  - crates/wunder-runtime/src/services/tools/subagent_control/flow.rs
  - crates/wunder-runtime/src/services/tools/subagent_control/messaging.rs
  - crates/wunder-runtime/src/services/tools/subagent_control/targeting.rs
  - crates/wunder-runtime/src/services/subagents/context.rs
  - crates/wunder-runtime/src/services/subagents/tree.rs
  - crates/wunder-runtime/src/services/runtime/thread/mailbox.rs
  - crates/wunder-runtime/src/services/tools/catalog.rs
updated_at: 2026-10-13
---

# 子智能体控制

规范名：`子智能体控制`　别名：`subagent_control`（`crates/wunder-runtime/src/services/tools/catalog.rs:1227`）。

子智能体是**主智能体临时创建的工作单元**：一次派生一个任务线程，任务结束即回收。协作形态只有这一种——**蜂群编排（蜂群工具、蜂房、编排态、team run、蜂群包、广场）已全链路移除**，仓库里不存在对应实现，任何文档与调用都不应再引用它。

## 什么时候用

- 一块工作可以独立完成、结果只需要摘要回来：`spawn`。
- 同一目标要多路并行尝试或交叉复核：`batch_spawn`。
- 已经在跑的派生要等结果、看状态、纠偏、打断或关闭：`wait` / `status` / `send` / `interrupt` / `close`。
- 子智能体自己要向父线程回话：`report`（只有子智能体能用）。

线程本身的管理（新建/切换/归档任务线程）用 [会话线程控制](/docs/zh-CN/tools/thread-control/)，不要用本工具。

## 动作与目标选择

`action` 必填，枚举十一个（`catalog.rs:813-939`），实现层另有别名（`subagent_control.rs:218-241`）：

| 动作 | 归一别名 | 语义 |
| --- | --- | --- |
| `list` | `sessions_list`、`列表` | 列出某父线程下的子智能体 |
| `history` | `sessions_history`、`历史` | 读单个子线程消息历史 |
| `send` | `sessions_send`、`发送` | 给单个子线程追加一轮指令 |
| `report` | — | 子 → 父回报（见下） |
| `spawn` | `sessions_spawn`、`派生` | 派生一个子智能体 |
| `batch_spawn` | `dispatch`、`批量派生` | 一批派生，带策略与剩余分支处置 |
| `status` | `inspect`、`状态` | 读目标集当前快照，不等待 |
| `wait` | `join`、`collect`、`等待` | 阻塞等到完成条件或超时 |
| `interrupt` | `cancel`、`stop`、`中断` | 取消运行，不改变会话状态 |
| `close` | `关闭` | 会话状态置 `closed`，可选级联 |
| `resume` | `reopen`、`恢复` | 状态置回 `active`；带 `message` 时等价于 `send` |

目标选择子（`subagent_control.rs:27-56`、`targeting.rs:553-588`）：`run_id` / `run_ids` / `session_id` / `session_ids` / `dispatch_id` / `parent_id`，全部为空时默认落在**当前会话作为父线程**（`catalog.rs:2294-2303` 的测试锁定了这个默认）。`limit` 1 起、上限 200、`wait`/`status` 默认 50；`run_ids + session_ids` 超过 200 直接报 `too many subagent targets`。

单目标动作（`history` / `send`）要求**恰好一个**子线程：会先按 id 精确解析，失败时在同父线程下找编辑距离 ≤2 的唯一近似 id，再退化为"只有一个子线程就选它"；仍不唯一就报错并提示用 `spawn`/`list` 返回的准确 id（`targeting.rs:91-183`、`223-278`、`509-537`）。

树边界：所有模型侧选择器都要过 `tree::authorize`——目标必须与调用者同根，且**根线程不能作为 worker 目标**（只读范围选择器例外）；深度上限 `MAX_DEPTH = 32`（`crates/wunder-runtime/src/services/subagents/tree.rs:6`、`57-70`）。

## 参数

模型侧 schema 关键字段（`catalog.rs:813-939`）：

| 字段 | 类型 / 上限 | 用于 |
| --- | --- | --- |
| `action` | string 枚举，必填 | 全部 |
| `task` / `tasks` | string / 数组最多 64 项 | `spawn` / `batch_spawn` |
| `tasks[].{task,label,agent_id,model,run_timeout_seconds,cleanup,fork_turns,context_summary}` | 子项对象，闭合 | `batch_spawn` 逐项覆盖 |
| `label`、`agent_id`、`model` | string | `spawn` / `batch_spawn` 批次默认 |
| `run_timeout_seconds` | number | 单次派生的运行超时；`spawn` 里同时充当同步等待时长 |
| `cleanup` | `keep` / `delete` | 子线程收尾方式，默认 `keep` |
| `fork_turns` | integer 0–16，默认 0 | 派生时复制最近 N 轮用户上下文 |
| `context_summary` | string ≤16384 UTF-8 字节 | 派生时附带的背景摘要 |
| `message` | string 1–20000 字节 | `send` / `report` / 带消息的 `resume` |
| `message_id` | string ≤128 | 重试用幂等 id，同一条消息才复用 |
| `timeout_seconds` | number | `send` 等待子线程回应的超时 |
| `wait_seconds` / `poll_interval_seconds` / `wait_mode` | number / number / `all`\|`any`\|`first_success` | `wait` |
| `strategy` | `parallel_all`\|`first_success`\|`review_then_merge` | `batch_spawn` |
| `remaining_action` | `keep`\|`interrupt`\|`close` | 提前完成后剩余分支处置 |
| `include_tools` | boolean | `history` 是否含工具消息 |
| `limit`、`parent_id`、`session_id(s)`、`run_id(s)`、`dispatch_id` | 见上 | 目标选择 |

## 派生与上下文继承

`fork_turns` 与 `context_summary` 是**引用数据，不改系统提示词**：`context::prepare` 最多复制 256 条历史项、总量 64 KiB，超出即置 `truncated`，并把父线程近 N 轮以"引用背景"的形式拼在任务文本前面（`crates/wunder-runtime/src/services/subagents/context.rs:8-79`）。`fork_turns > 16` 或摘要超 16384 字节直接失败；`fork_turns=0` 且摘要为空时不读历史。

工具系统提示词在该线程首次确定后保持冻结，长期记忆与背景注入只在初始化时发生一次——这正是上下文继承走"引用数据"而不是走 system 的原因。

`spawn`：子线程工具集继承父会话（`ChildSessionToolMode::InheritParentSession`，`crates/wunder-runtime/src/services/tools/session_tool_access.rs:15-19`）。`run_timeout_seconds ≤ 0` 时立即返回 `accepted`（后台跑），`> 0` 时同步等待，每 5 秒发一次 `progress` 事件，超时返回 `timeout`（`crates/wunder-runtime/src/services/tools/session_tool.rs:360-540`）。

`batch_spawn`（`subagent_control.rs:533-865`）：

1. 先对**每个**任务准备上下文，任一准备失败整批不派生（失败不会藏着半个批次）。
2. 生成 `dispatch_id`（`dispatch_<uuid>`），逐项生成 `run_id`（`run_<uuid>`），逐个派生；单项启动失败会回收该子线程并记为 `failed`。
3. `strategy` 决定完成条件：`parallel_all` / `review_then_merge` → 等全部；`first_success` → 首个成功即完成。
4. `remaining_action` 默认值随策略变：`first_success` 默认 `interrupt`，其余默认 `keep`。
5. `wait_seconds ≤ 0`（默认）不等待，直接返回 `accepted`（有启动失败时返回 `partial`）。

## 等待、回报与中断

`wait` 的轮询间隔归一到 0.2–5 秒，默认 1 秒（`subagent_control.rs:17-19`、`flow.rs:330-335`）。等待优先订阅运行信号，无信号时按间隔轮询；父线程被取消时等待直接报 `subagent wait interrupted`。等待可被三类条件终止：完成条件达成、超时、**父信箱收到消息**（`completed_reason` 为 `message_received`）（`flow.rs:67-154`）。

`send`：若子线程正在跑当前轮且信箱打开，消息直接进当前轮（`delivery = queued_current_turn`）；否则走新一轮发送。目标是自己时报错 `send must target another worker`（`subagent_control.rs:504-531`）。

`report`：只允许 `spawned_by` 为 `model` 或 `subagent_control` 的临时子线程调用，且父线程必须 `active`；父线程空闲时通过自动唤醒请求投递，返回 `queued_next_turn` 与 `queue_id`；带同一 `message_id` 重试会命中已存在的排队消息而不重复入箱（`subagent_control/messaging.rs:70-138`）。

`interrupt` 只请求取消运行（结果为 `cancelling` 或 `unchanged`），不改会话状态；`close` 会把会话状态写成 `closed` 并顺带取消运行，`cascade=true` 时连同后代一起处理；`resume` 把状态改回 `active`（`subagent_control.rs:967-1047`）。

状态归一（`targeting.rs:1095-1149`）：有效状态取"运行记录终态 → 会话已关闭 → 运行时状态 → 记录状态"的顺序；终态集合为 `success`/`error`/`timeout`/`cancelled`/`failed`/`closed`/`idle`/`not_found`；面向协作视图另有映射（`queued→pending_init`、`cancelled→interrupted`、`closed→shutdown` 等）。

## 返回

统一骨架 `ok / action / state / summary / data`（`tools/subagent_control.rs:280-446`）：

- `list` → `state=completed`，`data.total` + `data.items[]`。
- `history` → `state=completed`，`data.session_id` + `data.messages`。
- `send` / `report` → 回执 `state=accepted`，`data.{session_id,message_id,delivery[,queue_id]}`。
- `interrupt` / `close` / `resume` → 有更新时 `state=completed`，零更新时 `state=noop`，`data.items[]` 带逐项结果。
- `status` / `wait` / `batch_spawn` → `state` 直接取汇总状态：`completed`（原 `ok`）、`partial`、`running`、`timeout`、`empty`、`error`；`running` / `timeout` 时额外带 `next_step_hint`，提示先 `wait`/`status`/`history` 再判完成。

条目为控长度做了压缩（`subagent_control.rs:244-278`）：`root_session_id`、`task_path`、`index`、`dispatch_id`、`run_id`、`session_id`、`status`、`terminal`、`failed`、`updated`、`agent_id`、`label`、`elapsed_s`、`result_preview`、`error`。等待/批次结果的 `data` 另有 `counts{total,done,success,failed,queued,running,selected,selected_success,selected_failed}`、`selected_items`、`remaining_action`、`remaining_active_total`、`settled_total`、`requested_total`、`accepted_total`、`startup_failed_total`、`winner_item`（`first_success`）。

条目里 `result_preview` 与状态消息被截到 160 字符（`SUBAGENT_SUMMARY_MAX_CHARS`，`subagent_control.rs:20`）。完整内容去 `history` 或该子线程的轨迹里取。

除返回值外，运行过程还会发事件：`subagent_dispatch_start`、`subagent_dispatch_item_update`、`subagent_dispatch_finish`、`subagent_interrupt`、`subagent_close`、`subagent_resume`、`subagent_status`、`progress`（`flow.rs:257-328`、`subagent_control.rs:988-1036`）。

## 失败与错误元数据

常见的直接错误：`subagent_control action is required`、`unknown subagent_control action: <值>`、`batch_spawn requires at least one task`、`batch_spawn accepts at most 64 tasks`、`fork_turns must be between 0 and 16`、`context_summary exceeds 16384 UTF-8 bytes`、`too many subagent targets`、`subagent target is required`、`subagent wait interrupted`、`message must contain 1..20000 UTF-8 bytes`、`message_id must contain 1..128 bytes`、`message source was interrupted`、`target is interrupted; wait for settlement`、`report is only available to a temporary child agent`、`parent session is closed`。

带 `ToolErrorMeta` 的失败会给出 `error_meta{code,hint,retryable,retry_after_ms}`（`crates/wunder-runtime/src/services/tools/tool_error.rs:4-50`）。

## 边界

- **批次上限 64**：schema `maxItems(64)` 与运行时校验一致，超了整批拒绝而不是截断。
- **信箱有界**：单子线程待发 64 条 / 256 KiB，回执缓存 256 条，全局信箱 8192 个；超限按拒绝处理（`mailbox.rs:9-13`）。
- **列表有界**：`limit` 上限 200，默认 50；级联收集后代也受同一 `limit` 约束（`targeting.rs:997-1033`）。
- 桌面模式把 `子智能体控制` 列为默认额外启用工具（`default_tool_profile.rs:33-35`）；非桌面形态需管理员勾选。
- 子智能体的模型可被 `model` 覆盖，智能体可被 `agent_id` 覆盖；两者都受当前用户的智能体归属校验。
- `cleanup=delete` 才清理派生线程；`keep` 保留，可继续 `history` 追溯。

## 相关页面

- 线程管理：[会话线程控制](/docs/zh-CN/tools/thread-control/)
- 工具与结果裁剪：[工具](/docs/zh-CN/tools/index/)
