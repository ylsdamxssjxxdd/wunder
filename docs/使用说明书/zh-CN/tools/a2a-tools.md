---
title: A2A 工具
summary: `a2a@service`、`a2a_observe`、`a2a_wait` 的当前返回结构。
read_when:
  - 你要调用外部 A2A 服务并跟踪任务状态
source_docs:
  - src/services/tools.rs
updated_at: 2026-04-10
---

# A2A 工具

A2A 现在是一组工具：

- `a2a@服务名`
- `a2a_observe`
- `a2a_wait`

## `a2a@服务名`

提交任务，通常是接收态：

```json
{
  "ok": true,
  "action": "a2a_send",
  "state": "accepted",
  "summary": "Submitted task task_xxx to A2A service helper.",
  "data": {
    "endpoint": "https://a2a.example.com",
    "service_name": "helper",
    "task_id": "task_xxx",
    "context_id": "ctx_xxx",
    "status": "submitted",
    "answer": null
  }
}
```

## `a2a_observe`

看当前快照：

```json
{
  "ok": true,
  "action": "a2a_observe",
  "state": "running",
  "summary": "Observed 2 A2A tasks; 1 still pending.",
  "data": {
    "tasks": [ ... ],
    "pending": [ ... ],
    "pending_total": 1,
    "timeout": false
  }
}
```

## `a2a_wait`

等待一段时间直到完成或超时：

```json
{
  "ok": true,
  "action": "a2a_wait",
  "state": "running",
  "summary": "Observed 2 A2A tasks; 1 still pending.",
  "data": {
    "tasks": [ ... ],
    "pending": [ ... ],
    "pending_total": 1,
    "elapsed_s": 1.25,
    "timeout": true
  },
  "next_step_hint": "Call a2a_wait again or inspect the pending tasks before assuming the A2A workflow is complete."
}
```

## 重点

- `a2a@服务名` 负责发起
- `a2a_observe` 负责看快照
- `a2a_wait` 负责轮询等待
