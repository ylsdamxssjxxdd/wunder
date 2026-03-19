---
title: 子智能体控制
summary: `子智能体控制` 面向单个子会话运行的发现、历史查看、发消息和派生，适合“盯住一个子运行”而不是大范围并发派发。
read_when:
  - 你要查看或操作某个子会话
  - 你要区分子智能体控制和智能体蜂群
source_docs:
  - src/services/tools/catalog.rs
  - src/services/tools.rs
---

# 子智能体控制

如果 `会话线程控制` 解决的是线程树，`子智能体控制` 解决的就是单个子运行本身。

## 核心动作

- `list`
- `history`
- `send`
- `spawn`

## 常用参数

- `action`
- `limit`
- `activeMinutes`
- `messageLimit`
- `parentId`
- `session_id`
- `sessionKey`
- `includeTools`
- `message`
- `timeoutSeconds`
- `task`
- `label`
- `agentId`
- `model`
- `runTimeoutSeconds`
- `cleanup`

## 它适合什么

- 列出某个父会话下的子运行
- 查看某个子会话历史
- 给指定子会话继续发消息
- 派生一个新的子运行

## 和其他协作工具的区别

- [会话线程控制](/docs/zh-CN/tools/thread-control/) 偏线程树结构。
- `子智能体控制` 偏单个子运行管理。
- [智能体蜂群](/docs/zh-CN/tools/agent-swarm/) 偏多目标、多智能体并发协作。

如果你已经知道要操作哪一个子会话，这页对应的工具最直接。

## 推荐路径

- 看子运行列表：`list`
- 查某个子运行内容：`history`
- 派生新子运行：`spawn`
- 继续追问已有子运行：`send`

## 你最需要记住的点

- `子智能体控制` 面向单个子会话，不是面向整个线程树。
- `spawn` 更像派生一次后台运行，`send` 更像继续驱动已有子会话。
- 它是单目标工具；多目标并发优先看智能体蜂群。

## 相关文档

- [会话线程控制](/docs/zh-CN/tools/thread-control/)
- [智能体蜂群](/docs/zh-CN/tools/agent-swarm/)
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)
