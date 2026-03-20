---
title: 渠道工具
summary: `渠道工具` 是 Wunder 的模型侧渠道入口，当前重点能力是联系人发现和渠道消息发送。
read_when:
  - 你要让模型主动发渠道消息
  - 你在区分渠道工具和渠道 Webhook / 管理端渠道治理
source_docs:
  - docs/API文档.md
  - src/services/tools/channel_tool.rs
  - docs/设计方案.md
---

# 渠道工具

`渠道工具` 让模型直接操作渠道侧联系人和消息发送。

## 当前动作

- `list_contacts`
- `send_message`

## `list_contacts`

适合：

- 查联系人
- 查可用账号
- 按关键字找目标会话或目标用户

常见参数：

- `channel`
- `account_id`
- `keyword`
- `offset`
- `limit`
- `refresh`

## `send_message`

适合：

- 向某个渠道用户或群组发消息
- 带附件发消息
- 在需要时等待投递结果

常见参数：

- `channel`
- `account_id`
- `to`
- `peer_kind`
- `thread_id`
- `text`
- `content`
- `attachments`
- `wait`
- `wait_timeout_s`

## 它和渠道 Webhook 的区别

可以这样记：

- [渠道 Webhook](/docs/zh-CN/integration/channel-webhook/)：接收入站
- `渠道工具`：模型侧主动发出站

两者属于同一渠道系统，但方向不同。

## 实施建议

- `渠道工具` 当前重点是联系人发现和发送消息。
- 它属于模型侧出站工具，不等于管理员侧渠道治理接口。
- 真正排查渠道故障时，还要一起看渠道运行态和 outbox。

## 延伸阅读

- [渠道 Webhook](/docs/zh-CN/integration/channel-webhook/)
- [渠道运行态](/docs/zh-CN/ops/channel-runtime/)
