---
title: 智能体蜂群
summary: 调度已存在智能体的多智能体协作工具。
read_when:
  - 你要调度当前用户已经拥有的其他正式智能体
source_docs:
  - src/services/tools.rs
updated_at: 2026-04-10
---

# 智能体蜂群

`agent_swarm` 和 `subagent_control` 最大的区别是：

- `subagent_control`：当前会话里临时派生子运行
- `agent_swarm`：调度用户已经存在的其他正式智能体

## 主要动作

- `list`
- `send`
- `batch_send`
- `spawn`
- `wait`
- `status`
- `history`

## `list`

```json
{
  "ok": true,
  "action": "list",
  "state": "completed",
  "summary": "Listed 4 swarm workers.",
  "data": {
    "total": 4,
    "items": [ ... ]
  }
}
```

## `spawn`

典型是异步接收态：

```json
{
  "ok": true,
  "action": "spawn",
  "state": "accepted",
  "summary": "Spawned swarm task ...",
  "data": {
    "task_id": "task_xxx",
    "run_id": "run_xxx",
    "session_id": "sess_xxx",
    "agent_id": "agent_research",
    "agent_name": "研究员",
    "created_session": true
  },
  "next_step_hint": "Use agent_swarm.wait or status/history before treating the worker result as final."
}
```

## `status`

```json
{
  "ok": true,
  "action": "status",
  "state": "completed",
  "summary": "Loaded swarm status for 研究员.",
  "data": {
    "agent": { ... },
    "session_total": 6,
    "active_session_total": 2,
    "running_session_total": 1,
    "lock_session_total": 0,
    "active_session_ids": ["sess_xxx"],
    "recent_sessions": [ ... ]
  }
}
```

## 状态语义

`agent_swarm` 常见 `state`：

- `completed`
- `accepted`
- `running`
- `timeout`
- `partial`
- `cancelled`
- `error`

看到 `accepted`、`running`、`timeout` 时，通常都不该直接当最终结果，而应继续 `wait` 或 `status/history`。
