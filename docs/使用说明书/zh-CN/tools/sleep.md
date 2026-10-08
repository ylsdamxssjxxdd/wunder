---
title: 会话让出
summary: `sessions_yield` 的语义与适用场景。
read_when:
  - 需要暂时让出当前轮次控制权、等待外部结果
source_docs:
  - src/services/tools/sessions_yield_tool.rs
updated_at: 2026-09-28
---

# 会话让出

`sessions_yield` 表示：**当前轮次让出控制权，不是最终回复。**

独立的 `sleep`（休眠等待）工具已移除：命令执行默认阻塞返回结果，等待后台命令请用 `command_session` 的 `yield_time_ms` 等待窗口，不要空转等待。

## 最小参数

```json
{
  "message": "已提交任务，等待外部结果"
}
```

## 成功返回

```json
{
  "ok": true,
  "action": "sessions_yield",
  "state": "yielded",
  "summary": "Yielded the current turn and is waiting.",
  "data": {
    "status": "yielded",
    "message": "已提交任务，等待外部结果"
  },
  "meta": {
    "turn_control": {
      "kind": "yield",
      "message": "已提交任务，等待外部结果"
    }
  }
}
```

## 使用场景

- 需要明确告诉系统“这轮先停在这里、等待外部恢复”时使用。
- 等待命令完成不需要它：`execute_command` 默认阻塞返回；后台命令用 `command_session` 轮询。
