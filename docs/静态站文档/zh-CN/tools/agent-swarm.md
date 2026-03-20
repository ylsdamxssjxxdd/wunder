---
title: 智能体蜂群
summary: `智能体蜂群` 面向多智能体协作，支持单目标发送、批量派发、等待聚合结果和按会话回看历史。
read_when:
  - 你要同时调度多个智能体
  - 你要查 `batch_send -> wait` 这条典型协作路径
source_docs:
  - src/services/tools/catalog.rs
  - src/services/tools.rs
  - docs/API文档.md
---

# 智能体蜂群

`智能体蜂群` 是 Wunder 里最接近“多智能体协作总线”的内置工具。

## 核心动作

- `list`
- `status`
- `send`
- `history`
- `spawn`
- `batch_send`
- `wait`

## 常用参数

- `agentId`
- `sessionKey`
- `message`
- `task`
- `tasks`
- `runIds`

其中：

- 单目标协作通常用 `send`
- 多目标并发派发通常用 `batch_send`
- 等待结果通常用 `wait`

## 推荐路径

最常见的路径是：

- `list`
- `send` 或 `batch_send`
- `wait`
- `history` 或 `status`

这条路径和 OpenClaw 文档里的“先派发，再等待，再取结果”很接近。

## 它适合什么

- 让不同智能体分别做研究、法务、财务、写作
- 把同一任务拆给多个角色并行处理
- 在主线程之外做批量协作

## 和子智能体控制的区别

- [子智能体控制](/docs/zh-CN/tools/subagent-control/) 偏单个子会话。
- `智能体蜂群` 偏多目标、多智能体并发协作。

如果你只盯一个子运行，不必上蜂群。

## 实施建议

- `智能体蜂群` 最核心的路径是 `list -> send/batch_send -> wait -> history/status`。
- `send` 适合单目标，`batch_send` 适合多目标。
- `wait` 的输入是运行 ID，不是普通会话标题。

## 延伸阅读

- [子智能体控制](/docs/zh-CN/tools/subagent-control/)
- [会话线程控制](/docs/zh-CN/tools/thread-control/)
- [蜂群协作](/docs/zh-CN/concepts/swarm/)
