---
title: 记忆管理
summary: `memory_manager` 的动作、记忆范围与返回结构。
read_when:
  - 你要增删改查长期记忆片段
source_docs:
  - src/services/tools/memory_manager_tool.rs
updated_at: 2026-04-10
---

# 记忆管理

`memory_manager` 已经统一到标准成功骨架。  
主要动作：

- `list`
- `recall`
- `add`
- `update`
- `delete`
- `clear`

## `list`

```json
{
  "ok": true,
  "action": "list",
  "state": "completed",
  "summary": "Listed 3 memory entries.",
  "data": {
    "count": 3,
    "items": [
      {
        "memory_id": "mem_xxx",
        "title": "用户偏好",
        "summary": "偏好简洁回答",
        "content": "用户希望回答简洁",
        "category": "tool-note",
        "tags": ["preference"],
        "status": "active",
        "updated_at": 1760000000
      }
    ],
    "agent_id": "agent_docs"
  }
}
```

## `add`

```json
{
  "ok": true,
  "action": "add",
  "state": "completed",
  "summary": "Saved a memory entry.",
  "data": {
    "memory_id": "mem_xxx",
    "saved": true,
    "agent_id": "agent_docs"
  },
  "next_step_hint": "..."
}
```

## `recall`

```json
{
  "ok": true,
  "action": "recall",
  "state": "completed",
  "summary": "Recalled 2 memory entries.",
  "data": {
    "query": "用户偏好",
    "count": 2,
    "items": [
      {
        "memory_id": "mem_xxx",
        "title": "用户偏好",
        "summary": "偏好简洁回答",
        "content": "用户希望回答简洁",
        "category": "tool-note",
        "tags": ["preference"],
        "status": "active",
        "updated_at": 1760000000,
        "why": "matched 用户偏好; in title/summary"
      }
    ],
    "agent_id": "agent_docs"
  }
}
```

## 重点

- 返回里会把 `agent_id` 直接补进 `data`
- `recall` 更偏向“给当前上下文提供回忆”
- 一些成功返回会带 `next_step_hint`，提醒只对新会话生效或只影响当前会话回忆语义
