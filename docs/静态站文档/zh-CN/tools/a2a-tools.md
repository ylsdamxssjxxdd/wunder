---
title: A2A 工具
summary: Wunder 会把每个已启用的 A2A 服务暴露成 `a2a@服务名` 工具，再配合 `a2a观察`、`a2a等待` 做状态跟踪。
read_when:
  - 你要调用外部 A2A 服务
  - 你要跟踪 A2A 任务的完成状态
source_docs:
  - src/services/tools/catalog.rs
  - src/services/tools.rs
  - docs/API文档.md
---

# A2A 工具

Wunder 的 A2A 能力不是一个固定工具名，而是一组工具：

- `a2a@服务名`
- `a2a观察`
- `a2a等待`

## `a2a@服务名`

每个启用中的 A2A service 都会成为一个可直接调用的工具名。

常用参数通常是：

- `content`
- `session_id`

调用后系统会把任务信息写入本地任务存储，并返回：

- `task_id`
- `context_id`
- `status`
- `answer`

## `a2a观察`

`a2a观察` 用来拿当前任务快照。

常用参数：

- `task_ids`
- `tasks`
- `endpoint`
- `service_name`
- `refresh`
- `timeout_s`

它适合“看一眼现在进行到哪了”。

## `a2a等待`

`a2a等待` 用来轮询直到任务完成或超时。

常用参数：

- `wait_s`
- `poll_interval_s`
- `task_ids`
- `tasks`
- `endpoint`
- `service_name`

它适合“先发任务，再等结果回来”。

## 推荐路径

最常见的使用顺序是：

- `a2a@服务名`
- `a2a观察`
- `a2a等待`

如果你只想拿最终结果，通常直接 `a2a@服务名 -> a2a等待` 就够了。

## 和 MCP 的区别

- A2A 更像“面向智能体服务”的任务调用。
- MCP 更像“面向工具服务器”的能力接入。

两者都会进入统一工具体系，但协议模型不同。

## 你最需要记住的点

- A2A 真正的执行入口是 `a2a@服务名`。
- `a2a观察` 看快照，`a2a等待` 做轮询等待。
- Wunder 会把任务状态写回本地存储，而不是只在内存里短暂保留。

## 相关文档

- [A2A 接口](/docs/zh-CN/integration/a2a/)
- [MCP 入口](/docs/zh-CN/integration/mcp-endpoint/)
- [蜂群协作](/docs/zh-CN/concepts/swarm/)
