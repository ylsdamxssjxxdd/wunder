---
title: 计划面板 / 问询面板 / 会话让出
summary: `计划面板`、`问询面板`、`会话让出` 三个交互控制工具的动作、参数、返回与结束轮次语义。
read_when:
  - 智能体要多步任务里维护可见的执行计划
  - 需要在动手前向用户征询路线或取舍
  - 需要主动让出当前轮次等待外部事件
source_docs:
  - crates/wunder-runtime/src/services/tools/panel_tools.rs
  - crates/wunder-runtime/src/services/tools/sessions_yield_tool.rs
  - crates/wunder-runtime/src/services/tools/catalog.rs
  - crates/wunder-runtime/src/orchestrator/execute.rs
  - crates/wunder-runtime/src/services/default_tool_profile.rs
updated_at: 2026-10-13
---

# 计划面板 / 问询面板 / 会话让出

三个工具同属"交互控制"类，都不产生业务副作用，只改变前端呈现或当前轮次的结束方式。

| 规范名 | 别名 |
| --- | --- |
| `计划面板` | `update_plan` |
| `问询面板` | `question_panel`、`ask_panel` |
| `会话让出` | `sessions_yield`、`yield` |

别名归一见 `crates/wunder-runtime/src/services/tools/catalog.rs:1167-1176`。

> 旧页面里的 `a2ui` 工具已删除，本页不再描述它。独立的"休眠等待"工具也已删除，等待语义由 `会话让出` 承担。

## 计划面板

**什么时候用**：任务需要多步、且用户要看进度时用它建立并维护执行清单。一次性就能做完的事不要建计划。

**参数**（`catalog.rs:222-246`）：

| 字段 | 类型 | 必填 | 约束 |
| --- | --- | --- | --- |
| `explanation` | string | 否 | 计划变更说明 |
| `plan` | 数组 | 是 | 1–12 项，项为闭合对象 `{step, status}` |
| `plan[].step` | string | 是 | 空白项被丢弃 |
| `plan[].status` | string | 否 | `pending` / `in_progress` / `completed`；实现另接受 `in-progress`、`done`、`complete` 等归一（`panel_tools.rs:75-87`） |

`plan` 为空或全部项被丢弃时报错（`tool.plan.plan_required`）。

**语义**：全量替换，不是增量补丁。**同一步只允许一个 `in_progress`**：出现第二个时它被降级为 `pending`（`panel_tools.rs:35-41`）。

**返回**：统一骨架 `ok=true / action=plan_update / state=completed / summary / data{status:"ok"}`。结果里不回显计划内容，计划通过 `plan_update` 事件推给前端（`panel_tools.rs:58-66`）。

## 问询面板

**什么时候用**：存在多条合理路线、需要用户拍板再动手时。开面板就是**把当前轮次交给用户**，不要再在同一轮里继续假设答案。

**参数**（`catalog.rs:247-289`）：

| 字段 | 类型 | 必填 | 约束 |
| --- | --- | --- | --- |
| `questions` | 数组 | 是 | 1–4 项，项为闭合对象 |
| `questions[].question` | string | 是 | 留空时套用默认问句 |
| `questions[].options` | 数组 | 是 | 1–4 项，项为闭合对象 `{label, description?, recommended?}`；也接受纯字符串选项 |
| `questions[].multiple` | boolean | 否 | 默认 `false` |

兼容旧单题扁平写法：顶层 `question`/`prompt`/`title`/`header` + `options`/`routes`/`choices` + `multiple`/`allow_multiple`/`multi`（`panel_tools.rs:144-202`）。选项 `label` 含"推荐"或带 `recommended`/`preferred` 时标记为推荐项（`panel_tools.rs:232-253`）。

**裁剪与失败**：超过 4 题只取前 4；**没有任何选项的问题被整题丢弃**；全部丢弃后报 `tool.question_panel.options_required`。

**返回**：`ok=true / action=question_panel / state=awaiting_input / summary / data{questions}`。面板以 `keep_open=true` 发出。编排器在结果 `ok` 时把本轮收尾，最终答复文案取 `response.question_panel_waiting`，停止原因记 `question_panel`（`crates/wunder-runtime/src/orchestrator/execute.rs:1241-1245`）。也就是说：**开面板 = 本轮结束，等用户下一条消息**。

## 会话让出

**什么时候用**：本轮已无事可做，但要显式声明"现在让出、等待外部事件"，而不是硬编一段收尾话。它**不是最终回复的替代品**，也不是定时器。

**参数**（`catalog.rs:290-299`）：`message`（string，可选）—— 让出时向用户交代当前状态；留空时取默认等待文案（`sessions_yield_tool.rs:75-81`）。schema 关闭额外字段。

**返回**：`ok=true / action=sessions_yield / state=yielded / summary / data{status:"yielded", message}`，并额外挂一个 `meta.turn_control = {kind:"yield", message}`（`sessions_yield_tool.rs:22-40`、`66-73`）。

**结束语义**：编排器识别 `meta.turn_control.kind=yield` 后，把本轮答案设为该 `message`，停止原因记 `yield`，并写入停止元数据 `{type:"sessions_yield", status:"yielded", message}`（`orchestrator/execute.rs:1247-1262`）。

**形态差异**：桌面模式（蜂窝）把 `会话让出` 列为隐藏工具，不对智能体开放（`crates/wunder-runtime/src/services/default_tool_profile.rs:22-31`）。定时唤醒由 [定时任务](/docs/zh-CN/tools/schedule-task/) 负责。

## 失败

三者都走统一失败骨架：`ok=false` + `error` + `data.error_meta{code,hint,retryable,retry_after_ms}`（`crates/wunder-runtime/src/services/tools/tool_error.rs:4-50`）。最常见的失败是参数校验：`计划面板` 的空计划、`问询面板` 的无可用选项、以及 JSON 参数形态不对导致的反序列化错误。

## 边界

- 面板不鉴权、不审批：它们是呈现与轮次控制，写文件/执行命令的审批档位与此无关。
- 计划与问询的结果都很薄，目的是少占上下文；不要把详细内容塞进 `explanation` 让模型重复读回。
- 前端断流时面板事件可能不落渲染，但**轮次结束语义仍然生效**——问询/让出后不要期待同一轮继续。
- 计划项上限 12、问题上限 4、每题选项上限 4，超出部分在 schema 层就被拒绝或被裁剪。

## 相关页面

- 结果如何进入模型输入与展示：[工具](/docs/zh-CN/tools/index/)
- 派生与等待：[子智能体控制](/docs/zh-CN/tools/subagent-control/)
