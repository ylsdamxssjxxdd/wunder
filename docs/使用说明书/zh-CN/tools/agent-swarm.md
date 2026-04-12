---
title: 智能体蜂群
summary: 调度已存在智能体的多智能体协作工具。
read_when:
  - 你要调度当前用户已经拥有的其他正式智能体
source_docs:
  - src/services/tools.rs
updated_at: 2026-04-12
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

## 线程策略

- 默认是 `threadStrategy=fresh_main_thread`：工蜂收到任务后会新建一个干净线程，并把它绑定成自己的新主线程。
- 当你需要让工蜂继续在它自己的长期主线程里沉淀输出时，可以改用 `threadStrategy=main_thread`，或传 `reuseMainThread=true`。
- `main_thread` 语义是：优先复用工蜂当前主线程；如果它还没有主线程，系统会先创建一个再派工。
- `send` / `batch_send` 如果显式提供了 `sessionKey`，会优先使用那个指定线程；这时 `threadStrategy` 只作为补充信息，不再改变目标线程。
- `spawn` 也支持同样的线程策略参数，本质上会把策略透传给底层派工。

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
