---
title: 子智能体控制
summary: `subagent_control` 的动作、等待语义、状态语义与返回结构。
read_when:
  - 用户要在当前会话内派生子智能体临时工作
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
- 有 `next_step_hint` 时，说明系统明确希望用户继续跟进而不是直接收尾

## 保留与复用子智能体

右侧「子智能体」区域保留当前主会话创建的子智能体，不占用工作线程列表。点击条目可查看历史；已完成或已中断的子智能体仍可用于后续任务。

停止主智能体会同时中断其后台和多级子智能体，并保留历史。用户继续聊天后，主智能体先用 `list` 查看，再按需 `send(session_id,message)` 或 `resume(session_id,message)` 恢复工作或分派新任务。未被选中的子智能体保持待命。`resume` 不带消息只重新开放线程；运行中的子任务可以通过 send 追加指导；中断尚未收敛或线程正在收尾时应等待后重试。


## 执行中的指导与汇报

- 主智能体可用 `send(session_id,message)` 向运行中的子智能体补充约束，消息在下一次模型动作边界进入上下文，无需先中断。
- 子智能体可用 `report(message)` 报告阶段发现或阻塞，同时继续自己的工作。父智能体运行中接收消息，空闲时通过任务队列唤醒。
- `accepted` 只表示接收；`queued_current_turn` 表示当前轮稍后应用，`queued_next_turn` 表示持久排队。取消或异常可使尚未应用的消息失效。
- 重试可传 `message_id`；相同 ID 必须保持内容一致。运行中去重限当前接收轮最近 256 条，跨已结束轮次 send 视为新任务。
- 每条消息最多 20000 UTF-8 字节。消息过长、收件箱已满或线程正收尾会明确报错，缩短内容或稍后重试。
- `wait` 期间收到汇报或指导会提前返回 `completed_reason=message_received`，以便主智能体处理消息；这不代表子任务完成。
- 超长完成通知自动保留简短摘要和运行引用，完整结果仍在子线程历史中，可用 `status` / `history` 查询。
