---
title: 智能体蜂群
summary: 智能体蜂群面向多智能体协作，支持单目标发送、批量派发、等待聚合结果和按会话回看历史。
read_when:
  - 你要同时调度多个智能体
  - 你要查 `batch_send -> wait` 这条典型协作路径
source_docs:
  - docs/API文档.md
  - src/services/tools/catalog.rs
updated_at: 2026-04-10
---

# 智能体蜂群

多智能体并发协作工具。

## 先判断适不适合用它

`智能体蜂群` 只管理当前用户名下**已存在的其他智能体**。

如果你只是想给当前会话临时派生一个子运行，不要用这里，改看 [子智能体控制](/docs/zh-CN/tools/subagent-control/)。

别名：

- `agent_swarm`
- `swarm_control`

## 支持的动作

| 动作 | 用途 | 常用参数 |
|------|------|----------|
| `list` | 列出可用智能体 | - |
| `send` | 向单个智能体发送任务 | `agentId/agentName/name/sessionKey`、`message` |
| `batch_send` | 批量并发派发 | `tasks[]` |
| `wait` | 等待一组运行结果 | `runIds` |
| `status` | 查看运行状态快照 | `runIds` |
| `history` | 查看某个工蜂线程历史 | `sessionKey` |
| `spawn` | 让已存在智能体继续派生运行 | `agentId/agentName/name`、`task` |

## 最常用路径

最推荐的调用顺序是：

1. `list`
2. `send` 或 `batch_send`
3. `wait`
4. 必要时再 `status` 或 `history`

如果是多工蜂协作，优先：

1. `batch_send`
2. `wait`

## `send` 的关键语义

`send` 需要：

- 一个目标：`agentId`、`agentName`、`name` 或 `sessionKey`
- 一条消息：`message`

最重要的行为约定是：

- 如果**没有显式提供** `sessionKey`，系统会为目标工蜂新建线程
- 这个新线程会绑定成该工蜂新的主线程

这样做是为了让工蜂上下文保持干净。

只有你显式提供 `sessionKey` 时，系统才会复用指定线程。

### 示例：单目标发送

```json
{
  "action": "send",
  "agentId": "researcher",
  "message": "研究这个技术方案的可行性"
}
```

## `batch_send` 的关键语义

`batch_send` 适合一次并发派给多个工蜂。

每个 `tasks[]` 项都需要：

- 目标智能体或目标线程
- `message`

### 示例：批量派发

```json
{
  "action": "batch_send",
  "tasks": [
    {
      "agentId": "researcher",
      "message": "整理背景资料"
    },
    {
      "agentId": "writer",
      "message": "基于背景资料写初稿"
    },
    {
      "agentId": "reviewer",
      "message": "准备质量检查清单"
    }
  ]
}
```

## `wait` 怎么理解

`wait` 用于等待已派发运行收敛。

它的输入是：

- `runIds`

不是会话标题，也不是智能体名称。

### 示例：等待结果

```json
{
  "action": "wait",
  "runIds": ["run-1", "run-2", "run-3"]
}
```

等待语义分三种：

- 显式传 `0`：立即返回当前快照
- 显式传正数：等待指定超时
- 不传等待参数：走系统默认超时

## `spawn` 什么时候用

`spawn` 仍然面向已存在智能体，但它更像“继续派生一段运行”。

它需要：

- `agentId`、`agentName` 或 `name`
- `task`

### 示例：继续派生

```json
{
  "action": "spawn",
  "agentName": "researcher",
  "task": "继续分析这份方案的风险"
}
```

## 与子智能体控制的区别

| 工具 | 适用场景 |
|------|----------|
| [子智能体控制](/docs/zh-CN/tools/subagent-control/) | 当前会话内的临时子运行 |
| 智能体蜂群 | 调度当前用户已有的其他智能体 |

## 常见误区

### 误区一：`send` 一定会复用老线程

不会。

默认是新建线程并切成目标工蜂的主线程，只有显式传 `sessionKey` 才会复用。

### 误区二：`wait` 的输入是会话标题

不是。

`wait` 看的是 `runIds`。

### 误区三：蜂群和子智能体是一回事

不是。

蜂群调的是已有智能体，子智能体是临时派生运行。

## 延伸阅读

- [蜂群协作](/docs/zh-CN/concepts/swarm/)
- [子智能体控制](/docs/zh-CN/tools/subagent-control/)
- [会话线程控制](/docs/zh-CN/tools/thread-control/)
