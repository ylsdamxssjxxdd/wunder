---
title: 睡眠与让出
summary: `sleep` 与 `sessions_yield` 的语义区别。
read_when:
  - 你要等待一段时间，或暂时让出当前轮次控制权
source_docs:
  - src/services/tools/sleep_tool.rs
  - src/services/tools/sessions_yield_tool.rs
updated_at: 2026-04-10
---

# 睡眠与让出

这里其实有两个不同工具：

- `sleep`
- `sessions_yield`

它们不要混用。

## `sleep`

### 最小参数

```json
{
  "seconds": 1.5
}
```

### 成功返回

```json
{
  "ok": true,
  "action": "sleep",
  "state": "completed",
  "summary": "Slept for 1.5 seconds.",
  "data": {
    "requested_seconds": 1.5,
    "elapsed_ms": 1502,
    "reason": null
  }
}
```

它表示：**当前轮次里真的阻塞等待了一段时间。**

## `sessions_yield`

### 最小参数

```json
{
  "message": "已提交任务，等待外部结果"
}
```

### 成功返回

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

它表示：**当前轮次让出控制权，不是最终回复。**

## 怎么选

- 只是轮询间隔：`sleep`
- 需要明确告诉系统“这轮先停在这里”：`sessions_yield`
