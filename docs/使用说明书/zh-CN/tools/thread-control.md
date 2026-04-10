---
title: 会话线程控制
summary: `thread_control` 的动作、线程关系与返回结构。
read_when:
  - 你要创建、切换、归档或设置主线程
source_docs:
  - src/services/tools/thread_control_tool.rs
updated_at: 2026-04-10
---

# 会话线程控制

`thread_control` 管的是当前用户与智能体的会话线程，而不是子智能体运行控制。

## 主要动作

- `list`
- `info`
- `create`
- `switch`
- `back`
- `update_title`
- `archive`
- `restore`
- `set_main`

## 返回骨架

它成功时统一是：

```json
{
  "ok": true,
  "action": "create",
  "state": "completed",
  "summary": "Created a new thread.",
  "data": { ... }
}
```

有些动作还会附带 `next_step_hint`。

## `list`

典型返回：

```json
{
  "ok": true,
  "action": "list",
  "state": "completed",
  "summary": "Listed 5 threads.",
  "data": {
    "scope": "branch",
    "status": "active",
    "items": [
      {
        "id": "sess_xxx",
        "title": "工具文档更新",
        "status": "active",
        "created_at": "2026-04-10T10:00:00+08:00",
        "updated_at": "2026-04-10T10:03:00+08:00",
        "last_message_at": "2026-04-10T10:03:00+08:00",
        "agent_id": "agent_docs",
        "parent_session_id": null,
        "spawn_label": null,
        "is_main": true,
        "runtime_status": "idle"
      }
    ]
  }
}
```

## `create`

重点不只是新建，还可以：

- `switch`
- `set_main`

也就是新建后是否立刻切过去、是否设置为主线程。

## `set_main`

这个动作很重要。它会把某个会话线程绑定为当前智能体的一等现实主线程。

## 和 `subagent_control` 的区别

- `thread_control`：管会话线程本身
- `subagent_control`：管临时派生的子智能体运行

如果你想管理主线程、分支线程、归档状态，用 `thread_control`。  
如果你想等子智能体跑完、打断它、看它历史，用 `subagent_control`。
