---
title: 子智能体控制
summary: `subagent_control` 的动作、等待语义、状态语义与返回结构。
read_when:
  - 你要在当前会话内派生子智能体临时工作
source_docs:
  - src/services/tools/subagent_control.rs
updated_at: 2026-04-10
---

# 子智能体控制

`subagent_control` 现在是一个明确的多动作工具，不再只是旧版的简单“拉子线程”。

## 适用场景

它适合：

- 在当前主智能体会话里临时派生子智能体
- 查看、等待、打断、关闭、恢复这些子运行
- 做一轮或多轮派生协作

它不适合：

- 调度用户已经存在的其他正式智能体  
那是 [智能体蜂群](/docs/zh-CN/tools/agent-swarm/)

## 主要动作

- `list`
- `history`
- `send`
- `spawn`
- `batch_spawn`
- `status`
- `wait`
- `interrupt`
- `close`
- `resume`

## `spawn`

用于拉起新的子智能体运行。

典型返回会是“已接收但尚未结束”：

```json
{
  "ok": true,
  "action": "spawn",
  "state": "accepted",
  "summary": "Spawned child run ...",
  "data": {
    "run_id": "run_xxx",
    "session_id": "sess_xxx",
    "status": "accepted"
  },
  "next_step_hint": "Use subagent_control.wait/status/history before treating unfinished child runs as complete."
}
```

## `list`

```json
{
  "ok": true,
  "action": "list",
  "state": "completed",
  "summary": "Found 3 child sessions.",
  "data": {
    "total": 3,
    "items": [
      {
        "dispatch_id": "dispatch_xxx",
        "run_id": "run_xxx",
        "session_id": "sess_xxx",
        "status": "running",
        "terminal": false,
        "failed": false,
        "agent_id": "worker-a",
        "label": "检索资料",
        "elapsed_s": 12.3,
        "result_preview": null,
        "error": null
      }
    ]
  }
}
```

## `history`

```json
{
  "ok": true,
  "action": "history",
  "state": "completed",
  "summary": "Loaded 18 messages from child session history.",
  "data": {
    "session_id": "sess_xxx",
    "messages": [ ... ]
  }
}
```

## `status` / `wait`

这两个动作最关键。

### `status`

看快照，不阻塞：

```json
{
  "ok": true,
  "action": "status",
  "state": "running",
  "summary": "1 child runs are still active.",
  "data": {
    "status": "running",
    "items": [ ... ],
    "selected_items": [ ... ]
  }
}
```

### `wait`

轮询等待，会根据结果变成：

- `completed`
- `running`
- `timeout`
- `partial`

并且常带：

```json
{
  "next_step_hint": "Use subagent_control.wait/status/history before treating unfinished child runs as complete."
}
```

## `interrupt` / `close` / `resume`

这几个动作返回的核心是“哪些子会话被更新了”：

```json
{
  "ok": true,
  "action": "interrupt",
  "state": "completed",
  "summary": "interrupt updated 1 child sessions.",
  "data": {
    "updated_total": 1,
    "items": [
      {
        "session_id": "sess_xxx",
        "status": "cancelling"
      }
    ]
  }
}
```

## 重点理解

- `accepted` 不等于完成
- `status` 是看快照
- `wait` 才是等待收敛
- 有 `next_step_hint` 时，说明系统明确希望你继续跟进而不是直接收尾
