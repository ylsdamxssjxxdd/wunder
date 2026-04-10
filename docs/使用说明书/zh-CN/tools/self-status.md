---
title: 自我状态
summary: `self_status` 用于查看当前会话、模型、事件与线程运行态。
read_when:
  - 你要查看当前会话到底跑到了哪一步
source_docs:
  - src/services/tools/self_status_tool.rs
updated_at: 2026-04-10
---

# 自我状态

`self_status` 是一个诊断型工具。  
它和大多数工具不同，成功时直接返回状态对象，不包统一的 `ok/action/state/summary/data`。

## 最小参数

```json
{
  "detail_level": "standard"
}
```

## 常用参数

- `detail_level`：`basic` / `standard` / `full`
- `include_events`
- `events_limit`
- `include_system_metrics`

## 返回结构

```json
{
  "tool": "self_status",
  "detail_level": "standard",
  "session": {
    "user_id": "user_xxx",
    "session_id": "sess_xxx",
    "workspace_id": "user_xxx",
    "agent_id": "agent_docs",
    "is_admin": false
  },
  "model": {
    "active": { ... },
    "configured_default": { ... }
  },
  "context": {
    "context_occupancy_tokens": 12345,
    "context_overflow": false,
    "monitor_context_tokens": 12000,
    "monitor_context_tokens_peak": 18000
  },
  "monitor": {
    "status": "running",
    "stage": "tool_call",
    "summary": "正在等待子智能体结果",
    "updated_time": 1760000000
  },
  "thread": { ... },
  "rounds": {
    "user_rounds": 3,
    "model_rounds_peak": 12,
    "event_user_round_peak": 3
  },
  "events": {
    "total": 48,
    "last_event_id": 128,
    "counts": {
      "llm_request": 5,
      "tool_call": 12,
      "tool_result": 12,
      "approval_request": 0,
      "approval_resolved": 0
    },
    "by_type": { ... },
    "recent_limit": 20,
    "recent": [ ... ]
  },
  "system_metrics": { ... }
}
```

## 重点

- `detail_level=full` 时默认会带更多事件和系统指标
- `events.recent` 适合看最近发生了什么
- `thread` 和 `monitor` 适合看当前线程是否还在运行、卡在哪个阶段
