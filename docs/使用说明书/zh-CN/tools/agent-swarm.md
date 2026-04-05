---
title: 智能体蜂群
summary: 智能体蜂群面向多智能体协作，支持单目标发送、批量派发、等待聚合结果和按会话回看历史。
read_when:
  - 你要同时调度多个智能体
  - 你要查 batch_send -&gt; wait 这条典型协作路径
source_docs:
  - src/services/tools/catalog.rs
  - src/services/tools.rs
---

# 智能体蜂群

多智能体并发协作工具。

---

## 功能说明

`智能体蜂群` 是 Wunder 里最接近「多智能体协作总线」的内置工具。

**别名**：
- `agent_swarm`
- `swarm_control`

---

## 参数说明

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `action` | string | ✅ | 要执行的动作 |
| 其他参数 | - | ❌ | 根据 action 不同而不同 |

---

## 支持的动作

| 动作 | 说明 | 常用附加参数 |
|------|------|--------------|
| `list` | 列出可用智能体 | - |
| `status` | 查看运行状态 | `runIds` |
| `send` | 单目标发送 | `agentId`, `sessionKey`, `message`, `task` |
| `history` | 查看历史 | `sessionKey` |
| `spawn` | 派生运行 | `agentId`, `task`, `label` |
| `batch_send` | 批量派发 | `tasks` |
| `wait` | 等待结果 | `runIds` |

---

## 使用示例

### 列出可用智能体

```json
{
  "action": "list"
}
```

### 单目标发送

```json
{
  "action": "send",
  "agentId": "researcher",
  "task": "研究这个技术方案的可行性"
}
```

### 批量派发

```json
{
  "action": "batch_send",
  "tasks": [
    {
      "agentId": "researcher",
      "task": "研究技术方案"
    },
    {
      "agentId": "lawyer",
      "task": "审查法律风险"
    },
    {
      "agentId": "finance",
      "task": "评估成本"
    }
  ]
}
```

### 等待结果

```json
{
  "action": "wait",
  "runIds": ["run-1", "run-2", "run-3"]
}
```

### 查看运行状态

```json
{
  "action": "status",
  "runIds": ["run-1", "run-2", "run-3"]
}
```

---

## 完整智能体循环示例

```json
// 1. 列出可用智能体
{
  "action": "list"
}

// 2. 批量派发任务
{
  "action": "batch_send",
  "tasks": [
    {
      "agentId": "researcher",
      "task": "研究技术方案",
      "label": "技术研究"
    },
    {
      "agentId": "writer",
      "task": "撰写报告",
      "label": "报告撰写"
    }
  ]
}

// 3. 等待结果
{
  "action": "wait",
  "runIds": ["run-1", "run-2"]
}

// 4. 查看状态
{
  "action": "status",
  "runIds": ["run-1", "run-2"]
}

// 5. 查看历史
{
  "action": "history",
  "sessionKey": "session-123"
}
```

---

## 推荐路径

最常见的路径是：
1. `list` - 列出可用智能体
2. `send` 或 `batch_send` - 发送任务
3. `wait` - 等待结果
4. `history` 或 `status` - 查看结果

这条路径和「先派发，再等待，再取结果」很接近。

---

## 与子智能体控制的区别

| 工具 | 说明 | 适用场景 |
|------|------|----------|
| [子智能体控制](/docs/zh-CN/tools/subagent-control/) | 单个子会话 | 只盯一个子运行 |
| 智能体蜂群 | 多目标、多智能体并发 | 大范围并发派发 |

---

## 适用场景

✅ **适合使用智能体蜂群**：
- 让不同智能体分别做研究、法务、财务、写作
- 把同一任务拆给多个角色并行处理
- 在主线程之外做批量协作
- 单目标发送或批量派发
- 等待聚合结果

---

## 注意事项

1. **核心路径**：
   - 最核心的路径是 `list -> send/batch_send -> wait -> history/status`
   - 按这个顺序使用最顺畅

2. **单目标 vs 多目标**：
   - `send` 适合单目标
   - `batch_send` 适合多目标

3. **wait 的输入**：
   - `wait` 的输入是运行 ID
   - 不是普通会话标题

---

## 延伸阅读

- [子智能体控制](/docs/zh-CN/tools/subagent-control/)
- [会话线程控制](/docs/zh-CN/tools/thread-control/)
- [蜂群协作](/docs/zh-CN/concepts/swarm/)
