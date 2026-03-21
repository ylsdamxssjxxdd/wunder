---
title: 会话线程控制
summary: 会话线程控制负责线程树、主线程和派生会话管理，是 Wunder 会话结构层的正式工具。
read_when:
  - 你要管理线程树、主线程和派生会话
  - 你要区分线程结构控制和多智能体协作
source_docs:
  - src/services/tools/thread_control_tool.rs
  - src/services/tools/catalog.rs
---

# 会话线程控制

管理会话线程树的正式工具。

---

## 功能说明

会话树不是「前端自己记一下当前对话是哪个」，而是正式工具能力。

**别名**：
- `thread_control`
- `session_control`

---

## 参数说明

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `action` | string | ✅ | 要执行的动作 |
| 其他参数 | - | ❌ | 根据 action 不同而不同 |

---

## 支持的动作

| 动作 | 说明 | 附加参数 |
|------|------|----------|
| `list` | 列出所有线程 | - |
| `info` | 获取线程信息 | `thread_id` |
| `create` | 创建新线程 | `title`, `parent_id` |
| `switch` | 切换到线程 | `thread_id` |
| `back` | 返回父线程 | - |
| `update_title` | 更新线程标题 | `thread_id`, `title` |
| `archive` | 归档线程 | `thread_id` |
| `restore` | 恢复线程 | `thread_id` |
| `set_main` | 设为主线程 | `thread_id` |

---

## 使用示例

### 列出所有线程

```json
{
  "action": "list"
}
```

### 创建子线程

```json
{
  "action": "create",
  "title": "重构任务分支",
  "parent_id": "main-thread-123"
}
```

### 切换线程

```json
{
  "action": "switch",
  "thread_id": "thread-456"
}
```

### 设为主线程

```json
{
  "action": "set_main",
  "thread_id": "thread-456"
}
```

---

## 真正改变什么

它不是简单的聊天接口包装，而是直接改变：

- 线程结构
- 会话归属
- 主线程映射

---

## 与其他协作工具的区别

| 工具 | 说明 | 适用场景 |
|------|------|----------|
| 会话线程控制 | 线程树和主线程管理 | 把哪条线程设为当前主线 |
| [子智能体控制](/docs/zh-CN/tools/subagent-control/) | 单个子会话运行 | 把任务扔给另一个智能体去跑 |
| [智能体蜂群](/docs/zh-CN/tools/agent-swarm/) | 多智能体并发协作 | 并行派发多个智能体 |

---

## 适用场景

✅ **适合使用会话线程控制**：
- 新建子线程进行分支探索
- 在线程树里切换上下文
- 返回父线程继续之前的工作
- 归档或恢复历史线程
- 把某条线程设为主线程

---

## 注意事项

1. **不只是切对话**：
   - 线程控制是会话树工具
   - 会影响系统对主线程的映射

2. **set_main 的影响**：
   - 会改变系统对主线程的映射
   - 影响后续的会话归属

3. **工具分工**：
   - 子会话和蜂群协作是相关但独立的另外两类工具
   - 注意区分使用场景

---

## 延伸阅读

- [子智能体控制](/docs/zh-CN/tools/subagent-control/)
- [智能体蜂群](/docs/zh-CN/tools/agent-swarm/)
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)
- [蜂群协作](/docs/zh-CN/concepts/swarm/)
- [聊天会话](/docs/zh-CN/integration/chat-sessions/)
