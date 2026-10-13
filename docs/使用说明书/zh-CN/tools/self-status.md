---
title: 自我状态
summary: `自我状态`（别名 `self_status`）用于让智能体自查当前会话、模型、上下文占用、轮次与事件计数。
read_when:
  - 需要让智能体自查"我跑到哪一步了、上下文还剩多少、当前用哪个模型"
  - 排查会话卡住、模型不符预期、轮次异常时
source_docs:
  - crates/wunder-runtime/src/services/tools/self_status_tool.rs
  - crates/wunder-runtime/src/services/tools/catalog.rs
  - crates/wunder-runtime/src/services/default_tool_profile.rs
  - crates/wunder-runtime/src/services/tools/tool_error.rs
updated_at: 2026-10-13
---

# 自我状态

规范名：`自我状态`　别名：`self_status`（`crates/wunder-runtime/src/services/tools/self_status_tool.rs:7-8`）。模型侧看到的是规范名，英文别名经 `resolve_tool_name` 归一。

这是诊断型工具，不是执行型工具。**它不在默认工具集里**（`crates/wunder-runtime/src/services/default_tool_profile.rs:4-18`），需要管理员在能力面显式勾选。

## 什么时候用

- 用户或智能体要确认当前会话的模型、上下文占用、轮次进度。
- 需要判断是否该压缩上下文、是否该收尾。
- 需要拿到事件计数（模型请求、工具调用、审批）来判断这一轮做了什么。

不要用它来做业务动作：它只读，不改变任何状态。

## 参数

模型侧 schema 只暴露三项（`crates/wunder-runtime/src/services/tools/catalog.rs:1117-1135`）：

| 字段 | 类型 | 必填 | 取值 / 上限 | 默认 |
| --- | --- | --- | --- | --- |
| `detail_level` | string | 否 | `basic` / `standard` / `full`；实现另接受 `lite`、`summary`、`detail`、`detailed`、`verbose` 并归一 | `standard` |
| `include_events` | boolean | 否 | 是否返回近期事件列表 | 仅 `full` 时为真 |
| `events_limit` | integer | 否 | 1–200 | 20 |

实现层还接受 `include_system_metrics`（boolean，默认仅 `full` 为真），但它不在模型侧 schema 中（`self_status_tool.rs:21-23`、`78-80`）。

`detail_level` 归一在 `DetailLevel::from_raw`：未识别值一律落回 `standard`（`self_status_tool.rs:33-40`）。

## 返回

**返回例外**：本工具不套统一骨架 `ok/action/state/summary/data`，成功时直接返回一个状态对象（`self_status_tool.rs:132-176`）。字段如下：

| 顶层键 | 内容 |
| --- | --- |
| `tool` | 固定为别名 `self_status` |
| `detail_level` | 归一后的档位 |
| `session` | `user_id` / `session_id` / `workspace_id` / `agent_id` / `is_admin` |
| `model.active` | 从监视事件里最后一条 `llm_request` 反推的实际模型（`model` / `provider` / `base_url` / `source` / `event_id`）；无事件时为 `null` |
| `model.configured_default` | 配置的默认模型档案（`profile` / `provider` / `model` / `base_url`）；`llm.default` 为空时为 `null` |
| `context` | `context_occupancy_tokens`、`context_overflow`、`monitor_context_tokens`、`monitor_context_tokens_peak` |
| `monitor` | `status` / `stage` / `summary` / `updated_time` |
| `thread` | 编排器的会话运行时快照；无编排器时为 `null` |
| `rounds` | `user_rounds`、`model_rounds_peak`、`event_user_round_peak` |
| `events` | `total`、`last_event_id`、`counts{llm_request,tool_call,tool_result,approval_request,approval_resolved}`、`by_type`；`include_events` 为真时追加 `recent_limit` 与 `recent` |
| `system_metrics` | 仅 `include_system_metrics` 为真时存在 |

`recent` 每项含 `event_id` / `timestamp` / `type` / `summary` / `user_round` / `model_round`；只有 `full` 档才带完整 `data`（`self_status_tool.rs:178-187`、`303-339`）。`basic` 档会删掉 `events.by_type`（`self_status_tool.rs:189-195`）。

## 失败

参数无法反序列化时错误文本原样上抛（`self_status_tool.rs:69-73`），执行层记为 `ok=false`；带错误元数据的失败会给出 `error_meta{code,hint,retryable,retry_after_ms}`（`crates/wunder-runtime/src/services/tools/tool_error.rs:4-50`）。监视器不可用、无编排器不是错误——相应字段返回 `null`。

## 边界

- 数据源是**内存中的运行态**：监视记录按会话取，事件来自该会话的监视缓冲，不是数据库台账。要看全量历史用线程轨迹，不要指望这里的事件数覆盖全周期。
- `events_limit` 只裁剪 `recent` 数组，`events.total` 仍是缓冲区分层里的全量计数。
- `model.active` 来自事件流，可能与配置默认不一致；这正是它存在的理由。
- `context` 两组数值口径不同：`context_occupancy_tokens` 来自工作区会话上下文记账，`monitor_context_tokens` 来自监视器，两者并列返回、不互相换算。
- 全形态可用（蜂窝 / 蜂巢 / 舵机 / 舰体），只读，不触发审批，也不进沙箱执行。

## 相关页面

- 线程级会话管理：[会话线程控制](/docs/zh-CN/tools/thread-control/)
- 工具总览与启用档位：[工具](/docs/zh-CN/tools/index/)
