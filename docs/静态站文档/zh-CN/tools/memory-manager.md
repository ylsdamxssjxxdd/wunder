---
title: 记忆管理
summary: `记忆管理` 是 Wunder 的结构化长期记忆工具，重点动作是 list、add、update、delete、clear 和 recall。
read_when:
  - 你要理解模型如何主动检索和维护记忆碎片
  - 你想知道 recall 为什么不会回写当前线程 prompt
source_docs:
  - docs/API文档.md
  - docs/设计方案.md
  - src/services/tools/memory_manager_tool.rs
---

# 记忆管理

`记忆管理` 不是“把一段文本塞进 prompt”，而是结构化长期记忆的工具入口。

## 当前动作

- `list`
- `add`
- `update`
- `delete`
- `clear`
- `recall`

其中最关键的通常是：

- `add`
- `update`
- `recall`

## `recall` 现在的定位

当前 `recall` 更强调：

- 轻量关键词召回

而不是：

- 每轮自动改写线程 system prompt

这点非常重要，因为 Wunder 的线程提示词一旦冻结，就不会在后续轮次被回写。

## 什么时候该用它

- 模型对某个用户偏好或历史事实没有把握
- 用户刚指出“你记错了”
- 当前问题需要查已有长期记忆碎片

## 常见误区

### 把 recall 当成 prompt 重写

不对。

它返回的是召回结果，不是直接重写当前线程的冻结提示词。

### 把记忆工具当普通笔记工具

也不对。

这里更关注结构化事实、标题、分类、标签和版本替代关系。

## 实施建议

- `记忆管理` 是结构化长期记忆工具，不是动态 prompt 改写器。
- `recall` 的目标是主动检索，不是回写当前线程。
- 记忆治理要和线程冻结规则一起理解。

## 延伸阅读

- [长期记忆](/docs/zh-CN/concepts/memory/)
- [提示词与技能](/docs/zh-CN/concepts/prompt-and-skills/)
- [额度与 Token 占用](/docs/zh-CN/concepts/quota-and-token-usage/)
