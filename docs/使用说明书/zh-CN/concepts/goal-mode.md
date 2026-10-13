---
title: 目标态
summary: 目标态是按模型轮次预算驱动的会话级持久目标：设定一次，智能体自动延续工作轮次，直到完成、受阻或预算用尽。
read_when:
  - 需要智能体跨多轮持续完成一个长任务，而不是问答一轮即止
  - 目标不推进、恢复会话后不再延续、或想弄清楚目标处于哪种状态
  - 判断该用目标态还是普通对话轮次
source_docs:
  - crates/wunder-runtime/src/services/goal/types.rs
  - crates/wunder-runtime/src/services/goal/driver.rs
  - crates/wunder-runtime/src/services/goal/tools.rs
  - crates/wunder-runtime/src/services/goal/prompt.rs
  - crates/wunder-runtime/src/api/chat_goal.rs
  - crates/wunder-core/src/storage_records.rs
updated_at: 2026-10-13
---

# 目标态

## 一句话模型

目标态 = 绑定在**会话**上的持久目标记录 + 一个只在**本进程内**生效的驱动器。驱动器在会话空闲时自动提交延续轮次（goal round），每轮消耗 1 次模型轮次预算；预算、相位或受阻条件决定它是否继续。

目标记录核心字段（`SessionGoalRecord`）：

| 字段 | 含义 |
| --- | --- |
| `objective` | 目标描述，上限 4000 字符 |
| `phase` | `active` / `paused` / `blocked` / `complete` 四选一 |
| `max_goal_rounds` | 自动延续轮次预算，创建时缺省 256 |
| `rounds_started` | 已启动的延续轮次，只增不减 |
| `revision` | 每次变更递增；更新必须携带 `goal_id` + 精确 `revision`（CAS 比对，旧值拒绝） |
| `blocked_code` / `blocked_message` | 受阻时的机器码与人类可读说明 |

## 适用与不适用

| 场景 | 建议 |
| --- | --- |
| 长任务、有明确完成条件、需要模型自主多轮推进 | 用目标态 |
| 一轮问答即可解决 | 直接发消息 |
| 需要用户频繁介入决策 | 直接对话，必要时配合问询面板 |
| 只想让当前这一轮做得更深 | 普通轮次即可，模型轮次本身没有硬性一步一答限制 |

## 状态机

| phase | 含义 | 进入条件 | 离开方式 |
| --- | --- | --- | --- |
| `active` | 可延续 | 创建目标，或 `resume` | pause / blocked / complete |
| `paused` | 挂起，人可随时恢复 | `update_goal action=pause`、`/goal pause`，或延续轮次被中断后自动落位 | `resume` |
| `blocked` | 受阻停止自动延续 | 同一阻塞条件持续至少 3 轮（`BLOCKED_AFTER_CONSECUTIVE_ROUNDS`）后由模型报告；或驱动器入队失败（`blocked_code=queue-failed`） | `resume`（需先解决阻塞条件） |
| `complete` | 达成 | 模型或人标记 `complete`，仅在目标真正达成时 | `/goal <新目标>` 重新开启 |

预算判断只看一条门：`phase=active` **且** 进程内激活状态为 armed **且** `rounds_started < max_goal_rounds`。三条同时满足才发起下一轮；人类消息始终优先，延续轮次不与人抢会话。

## 重要边界：没有 token 预算，没有冷却

- 预算单位是**模型轮次**，不是 token。系统不统计目标消耗了多少 token，也没有「预算耗尽进入 budget_limited 状态」这种设计。
- 轮次之间没有冷却计时；驱动器在会话空闲时立即评估是否发起下一轮。
- `blocked` 不等于「任务难」。当前指引明确：困难、不确定、还有剩余可用工作都不构成受阻理由，必须报告一个跨轮持续存在的具体阻塞条件。

## armed 状态不持久

armed 是进程内的激活标志，**不落库**。进程重启、会话恢复或分叉后，即使 phase 仍是 `active`，驱动器也不会自动延续——需要一次显式的 `update_goal action=resume`（或 `/goal resume`）重新 arm。判断「为什么目标不动了」时，先看 activation 是否为 disarmed。

## 三个工具与注入方式

模型侧操作目标是三个工具（运行时按会话注入，目录中以 `goal → update_goal` 提供英文别名归一）：

| 工具 | 作用 | 关键约束 |
| --- | --- | --- |
| `get_goal` | 读取当前目标（含 `goal_id`、`revision`） | `update_goal` 前必须先取权威值 |
| `create_goal` | 从人类直接请求推断并创建目标 | 参数 `objective` 必填，`max_goal_rounds` 可选 |
| `update_goal` | `edit` / `pause` / `resume` / `complete` / `blocked` | 需 `goal_id` + `revision`；`blocked` 在未满最低轮次数前被拒绝 |

权限靠宿主盖章（`__source=human` / `__goal_round`）判定：`edit/pause/resume` 只在人类直接发起的轮次中允许；`complete/blocked` 额外允许在本目标的自动延续轮次中报告。

蜂窝桌面端**不对用户开放选择**这三个工具（属于桌面隐藏内置工具），它们始终由运行时注入；蜂巢与舵机同样复用这套注入。

## 入口

| 入口 | 位置 |
| --- | --- |
| REST | `GET/PUT/DELETE /wunder/chat/sessions/{id}/goal`（用户面，需归属校验） |
| 斜杠命令 | `/goal [<objective>|clear|edit <objective>|pause|resume]` |
| 蜂巢 | 消息区目标条（`MessageGoalBar`） |
| 蜂窝 | 输入框上方目标横幅，支持行内编辑（`GoalBanner`） |
| 舵机 | `/goal` 命令 |
| 舰桥 | **无目标界面**（治理面不提供 goal 操作） |

## 常见误解

- **「目标会按 token 限额停下来」**——不会。停不停只看轮次预算、phase 和 armed 三件事。
- **「重启服务后活跃目标会自动继续」**——不会。armed 不持久，需 `resume` 重新 arm。
- **「模型可以随意把目标标成 blocked」**——不行。同一阻塞条件至少要持续 3 个延续轮次才允许报告受阻。
- **「只有一个叫 goal 的工具」**——现在是 `get_goal` / `create_goal` / `update_goal` 三个；旧文档中的单一 `goal` 工具（action=create/update）与 `[GOAL_CONTINUATION]` 注入标记均已不存在。
- **「在舰桥配置目标」**——舰桥没有 goal 面板，请使用蜂巢/蜂窝/舵机。
- **「清除目标会撤销已做的工作」**——不会。目标记录删除不影响已经落库的消息、工具结果与文件改动。

## 相关页

- [模型轮次与会话轮次](/docs/zh-CN/concepts/sessions-and-rounds/) — 预算消耗的最小单位
- [智能体循环](/docs/zh-CN/concepts/core-agent-loop/) — 延续轮次如何进入执行主循环
- [形象系统](/docs/zh-CN/concepts/companion/) — 运行状态在形象上的呈现
