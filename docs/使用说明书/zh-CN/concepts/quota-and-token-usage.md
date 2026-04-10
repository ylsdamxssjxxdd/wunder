---
title: "Token 账户与占用"
summary: "Wunder 用 Token 账户管理用户可支配余额，用 Token 占用观察上下文负载；这两者相关，但不是一回事。"
read_when:
  - "你在看 Token 余额、每日发放或会话资源占用"
  - "你要理解为什么系统强调 round_usage.total_tokens 与上下文占用"
source_docs:
  - "docs/API文档.md"
  - "docs/设计方案.md"
  - "docs/系统介绍.md"
---

# Token 账户与占用

Wunder 里最容易被误读的一件事，就是把 Token 账户、Token 消耗和供应商账单混成一件事。

## 本页重点

这页解释两个并列概念：

- 什么是用户的 Token 账户
- 什么是运行时的 Token 占用

## Token 账户是什么

可以把 Token 账户理解成用户在 Wunder 里的可支配货币余额：

- `token_balance`：当前余额，可以累计，也会随着模型消耗而减少
- `token_granted_total`：累计发放和奖励总额
- `token_used_total`：累计消耗总额
- `daily_token_grant`：当前用户每天应发放的 Token 数
- `last_token_grant_date`：最近一次完成每日发放的日期

当前默认规则是：

- 一级/二级/三级/四级用户每天分别发放 `100M / 50M / 10M / 1M` Token
- 每日发放会直接累积到 `token_balance`
- 模型调用按实际 `total_tokens` 扣减
- 用户升级时会额外奖励 Token，奖励同样直接入账到 Token 账户

所以这里的 Token 更接近系统内货币，而不是“一天一清零”的额度条。

## Token 占用是什么

Token 占用是运行时观测口径，回答的是：

- 当前这一轮请求到底占了多少上下文
- 这次请求实际消耗了多少 Token

它不是供应商账单口径，也不是用户余额本身。

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

如果你在做新接入，推荐直接消费这些显式别名：

- `context_occupancy_tokens`：当前上下文占用
- `request_consumed_tokens`：单次请求消耗
- `consumed_tokens`：聚合接口里的累计消耗

而 `context_usage` 这类字段更偏过程估算，适合观测，不适合当唯一准绳。

## Token 账户和 Token 占用不是一回事

可以这样理解：

- Token 账户：治理与结算口径，决定用户还剩多少可支配 Token
- Token 占用：运行观测口径，告诉你这轮上下文实际有多重

所以一个请求可能：

- 余额还够，但上下文已经很重
- 账单不算高，但线程治理已经开始吃紧

## 适用场景

- 监控里某些会话 token_usage 异常高
- 你在排查某个用户为什么余额不足
- 你在做压缩、裁剪和工具结果长度治理
- 你在解释“为什么管理员态和普通用户态口径不同”

## 实施建议

- 看用户还能不能继续使用时，优先看 Token 账户字段。
- Wunder 记录的当前上下文占用，优先看 `round_usage.total_tokens`。
- Wunder 记录的累计消耗，按每次请求的 `round_usage.total_tokens` 累加。
- 不要把单次调用 usage、上下文占用、累计消耗、Token 账户和供应商账单混成一个概念。

## 延伸阅读

- [流式事件参考](/docs/zh-CN/reference/stream-events/)
- [运维概览](/docs/zh-CN/ops/)
- [长期记忆](/docs/zh-CN/concepts/memory/)
