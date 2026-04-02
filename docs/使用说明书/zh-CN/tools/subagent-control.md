---
title: 子智能体控制
summary: 子智能体控制面向单个子会话运行的发现、历史查看、发消息和派生，适合盯住一个子运行而不是大范围并发派发。
read_when:
  - 你要查看或操作某个子会话
  - 你要区分子智能体控制和智能体蜂群
source_docs:
  - src/services/tools/catalog.rs
  - src/services/tools.rs
---

# 子智能体控制

管理单个子会话的工具。

---

## 功能说明

如果 `会话线程控制` 解决的是线程树，`子智能体控制` 解决的就是单个子运行本身。

**别名**：
- `subagent_control`

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
| `list` | 列出子会话 | `parentId`, `limit`, `activeMinutes` |
| `history` | 查看子会话历史 | `session_id`, `sessionKey`, `messageLimit` |
| `send` | 给子会话发消息 | `session_id`, `sessionKey`, `message`, `timeoutSeconds` |
| `spawn` | 派生新子会话 | `parentId`, `task`, `label`, `agentId`, `model`, `runTimeoutSeconds`, `cleanup` |

---

## 使用示例

### 列出子会话

```json
{
  "action": "list",
  "parentId": "main-session-123",
  "limit": 10,
  "activeMinutes": 60
}
```

### 查看子会话历史

```json
{
  "action": "history",
  "sessionKey": "sub-session-456",
  "messageLimit": 50
}
```

### 给子会话发消息

```json
{
  "action": "send",
  "sessionKey": "sub-session-456",
  "message": "请继续分析这个问题",
  "timeoutSeconds": 300
}
```

### 派生新子会话

```json
{
  "action": "spawn",
  "parentId": "main-session-123",
  "task": "分析这个代码的性能问题",
  "label": "性能分析",
  "agentId": "code-analyzer",
  "model": "gpt-4",
  "runTimeoutSeconds": 600,
  "cleanup": true
}
```

---

## 与其他协作工具的区别

| 工具 | 说明 | 适用场景 |
|------|------|----------|
| [会话线程控制](/docs/zh-CN/tools/thread-control/) | 线程树结构管理 | 管理线程树和主线程 |
| 子智能体控制 | 单个子会话管理 | 盯住一个子运行 |
| [智能体蜂群](/docs/zh-CN/tools/agent-swarm/) | 多智能体并发协作 | 大范围并发派发 |

---

## 推荐路径

- 看子运行列表：`list`
- 查某个子运行内容：`history`
- 派生新子运行：`spawn`
- 继续追问已有子运行：`send`

---

## 适用场景

✅ **适合使用子智能体控制**：
- 列出某个父会话下的子运行
- 查看某个子会话历史
- 给指定子会话继续发消息
- 派生一个新的子运行

---

## 注意事项

1. **单个子会话**：
   - 面向单个子会话，不是面向整个线程树
   - 如果已经知道要操作哪一个子会话，这个工具最直接

2. **spawn 与 send 的区别**：
   - `spawn` 更像派生一次后台运行
   - `send` 更像继续驱动已有子会话

3. **单目标工具**：
   - 它是单目标工具
   - 多目标并发优先看智能体蜂群

---

## 延伸阅读

- [会话线程控制](/docs/zh-CN/tools/thread-control/)
- [智能体蜂群](/docs/zh-CN/tools/agent-swarm/)
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)
