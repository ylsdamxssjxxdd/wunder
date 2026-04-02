---
title: "额度与 Token 占用"
summary: "Wunder 里的 token 统计重点是上下文占用，而不是直接等同于账单消耗。"
read_when:
  - "你在看监控、额度或会话资源占用"
  - "你要理解为什么系统强调 round_usage.total_tokens 与 context 占用"
source_docs:
  - "docs/API文档.md"
  - "docs/设计方案.md"
  - "docs/系统介绍.md"
---

# 额度与 Token 占用

Wunder 里最容易被误读的一件事，就是把 token 统计直接当成“花了多少钱”。

## 本页重点

这页只解释两个概念：

- 什么叫 Token 占用
- 它和额度、账单、监控之间是什么关系

## Token 占用不是账单口径

当前系统更强调：

- 模型这一轮实际接收了多少上下文

而不是：

- 供应商最终按什么账单规则计费

所以你看到的 `token_usage`，首先应该理解成运行时资源占用。

## 为什么要强调这个口径

因为 Wunder 的很多稳定性问题，本质都和上下文大小有关：

- 线程会不会过长
- 压缩是否及时
- 工具结果是否把上下文顶满
- 某类任务是否长期占着高输入窗口

如果只盯账单，很难指导系统治理。

## 当前最该看的字段

在监控和会话汇总里，最有意义的是：

- `round_usage.total_tokens`
- `token_usage.total_tokens`

其中：

- `round_usage.total_tokens` 表示**单轮请求完成后的实际上下文占用**，当前作为上下文占用的权威口径。
- `token_usage.total_tokens` 表示**单次模型调用的 usage 明细**；当一轮只发生一次模型调用时，它通常会和 `round_usage.total_tokens` 一致。

而 `context_usage` 这类字段更偏过程估算，适合观测，不适合当唯一准绳。

## 额度和 Token 占用不是一回事

可以这样理解：

- 额度：治理规则，决定某类用户或请求能不能继续跑
- Token 占用：运行观测，告诉你这轮上下文实际有多重

所以一个请求可能：

- 额度还够，但上下文已经很重
- 账单不算高，但线程治理已经开始吃紧

## 适用场景

- 监控里某些会话 token_usage 异常高
- 你在做压缩、裁剪和工具结果长度治理
- 你在解释“为什么管理员态和普通用户态口径不同”

## 实施建议

- Wunder 记录的当前上下文占用，优先看 `round_usage.total_tokens`。
- Wunder 记录的累计消耗，按每次请求的 `round_usage.total_tokens` 累加。
- 不要把单次调用 usage、上下文占用、累计消耗、额度和供应商账单混成一个概念。

## 延伸阅读

- [流式事件参考](/docs/zh-CN/reference/stream-events/)
- [运维概览](/docs/zh-CN/ops/)
- [长期记忆](/docs/zh-CN/concepts/memory/)
