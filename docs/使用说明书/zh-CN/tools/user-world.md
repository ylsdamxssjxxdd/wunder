---
title: 用户世界工具
summary: `用户世界工具` 面向 Wunder 系统内的注册用户目录和站内消息发送，不等同于外部渠道消息工具。
read_when:
  - 你要在 Wunder 内部查用户或发站内消息
  - 你要区分 `用户世界工具` 和 `渠道工具`
source_docs:
  - src/services/tools/catalog.rs
  - src/services/tools.rs
  - docs/API文档.md
---

# 用户世界工具

`用户世界工具` 主要做两件事：

- 查系统内用户
- 给系统内用户发消息

## 核心动作

- `list_users`
- `send_message`

## 常用参数

### `list_users`

- `keyword`
- `offset`
- `limit`

返回结果会包含用户目录里的基础信息，例如：

- `user_id`
- `username`
- `status`
- `unit_id`

### `send_message`

- `user_id`
- `user_ids`
- `content`
- `content_type`
- `client_msg_id`

`send_message` 至少要给出：

- 消息内容
- 一个目标用户或一组目标用户

## 一个很实用的细节

发送站内消息时，内容里的本地文件引用可以被系统识别并暂存。

也就是说，模型不只是能发纯文本，还能把工作区里的文件引用整理后一起带过去。

这也是结果里会出现 `staged_files` 的原因。

## 和渠道工具的区别

- `用户世界工具` 面向 Wunder 系统内部用户。
- [渠道工具](/docs/zh-CN/tools/channel/) 面向微信、Slack、Telegram 一类外部渠道。

如果目标是站内协作，优先看这页。

如果目标是跨外部 IM 平台发消息，优先看渠道工具。

## 实施建议

- `用户世界工具` 只处理系统内用户，不处理外部渠道联系人。
- `list_users` 查目录，`send_message` 发站内消息。
- 它支持把工作区文件引用整理成可发送的暂存文件。

## 延伸阅读

- [渠道工具](/docs/zh-CN/tools/channel/)
- [用户世界接入](/docs/zh-CN/integration/user-world/)
- [用户侧前端](/docs/zh-CN/surfaces/frontend/)
