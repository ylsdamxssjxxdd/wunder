---
title: 记忆管理
summary: 记忆管理是 Wunder 的结构化长期记忆工具，重点动作是 list、add、update、delete、clear 和 recall。
read_when:
  - 你要理解模型如何主动检索和维护记忆碎片
  - 你想知道 recall 为什么不会回写当前线程 prompt
source_docs:
  - src/services/tools/memory_manager_tool.rs
  - src/services/tools/catalog.rs
---

# 记忆管理

结构化长期记忆工具。

---

## 功能说明

`记忆管理` 不是「把一段文本塞进 prompt」，而是结构化长期记忆的工具入口。

**别名**：
- `memory_manager`
- `memory_manage`

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
| `list` | 列出记忆 | - |
| `add` | 添加记忆 | `title`, `content`, `category`, `tags` |
| `update` | 更新记忆 | `id`, `title`, `content`, `category`, `tags` |
| `delete` | 删除记忆 | `id` |
| `clear` | 清空记忆 | - |
| `recall` | 召回记忆 | `query`, `limit` |

---

## 使用示例

### 列出记忆

```json
{
  "action": "list"
}
```

### 添加记忆

```json
{
  "action": "add",
  "title": "用户偏好",
  "content": "用户喜欢使用深色主题，偏好简洁的界面设计",
  "category": "user_preference",
  "tags": ["theme", "ui", "preference"]
}
```

### 更新记忆

```json
{
  "action": "update",
  "id": "mem-123",
  "title": "用户偏好",
  "content": "用户喜欢使用深色主题，偏好简洁的界面设计，字体大小偏好 14px",
  "category": "user_preference",
  "tags": ["theme", "ui", "preference", "font"]
}
```

### 删除记忆

```json
{
  "action": "delete",
  "id": "mem-123"
}
```

### 召回记忆

```json
{
  "action": "recall",
  "query": "用户主题偏好",
  "limit": 5
}
```

---

## recall 的定位

当前 `recall` 更强调：
- 轻量关键词召回

而不是：
- 每轮自动改写线程 system prompt

这点非常重要，因为 Wunder 的线程提示词一旦冻结，就不会在后续轮次被回写。

---

## 适用场景

✅ **适合使用记忆管理**：
- 模型对某个用户偏好或历史事实没有把握
- 用户刚指出「你记错了」
- 当前问题需要查已有长期记忆碎片
- 保存重要的用户偏好、事实、上下文
- 按关键词召回相关记忆

---

## 常见误区

### 把 recall 当成 prompt 重写

❌ 不对。它返回的是召回结果，不是直接重写当前线程的冻结提示词。

### 把记忆工具当普通笔记工具

❌ 也不对。这里更关注结构化事实、标题、分类、标签和版本替代关系。

---

## 注意事项

1. **不是动态 prompt 改写器**：
   - 是结构化长期记忆工具
   - 不是动态 prompt 改写器

2. **recall 的目标**：
   - `recall` 的目标是主动检索
   - 不是回写当前线程

3. **线程冻结规则**：
   - 记忆治理要和线程冻结规则一起理解
   - 线程提示词一旦冻结就不会再被改写

---

## 延伸阅读

- [长期记忆](/docs/zh-CN/concepts/memory/)
- [提示词与技能](/docs/zh-CN/concepts/prompt-and-skills/)
- [额度与 Token 占用](/docs/zh-CN/concepts/quota-and-token-usage/)
