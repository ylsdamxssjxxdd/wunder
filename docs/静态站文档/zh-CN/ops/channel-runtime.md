---
title: 渠道运行态
summary: 渠道问题很多时候不在模型，而在账号、长连接、Webhook、outbox 和下载链接改写这条接入链路里。
read_when:
  - 你在排查飞书、微信、QQBot、WhatsApp、XMPP 等渠道异常
  - 你想知道管理员侧和用户侧分别有哪些渠道运行态接口
source_docs:
  - docs/API文档.md
  - docs/设计方案.md
  - docs/系统介绍.md
---

# 渠道运行态

很多渠道问题表面上像“模型没答出来”，其实更早就坏在接入层。

## 本页重点

这页只讲渠道运维要先看哪些面：

- 账号是否真的在线
- 长连接是否活着
- 入站有没有进来
- 出站有没有卡在 outbox

## 管理员侧最重要的入口

- `GET /wunder/admin/channels/accounts`
- `POST /wunder/admin/channels/accounts/batch`
- `DELETE /wunder/admin/channels/accounts/{channel}/{account_id}`
- `GET /wunder/admin/channels/accounts/{channel}/{account_id}/impact`
- `GET /wunder/admin/channels/bindings`
- `GET /wunder/admin/channels/user_bindings`
- `GET /wunder/admin/channels/sessions`

这组接口适合回答：

- 哪些账号异常
- 哪些绑定会受到影响
- 某渠道会话到底绑到了哪个 `user_id` / `agent_id`

## 用户侧运行日志入口

- `GET /wunder/channels/runtime_logs`
- `POST /wunder/channels/runtime_logs/probe`

这组接口适合回答：

- 当前用户可见账号最近有没有长连接失败、重连、运行告警
- 日志面板是不是被权限或过滤条件挡住了

## 什么时候不要先怪模型

如果出现这些症状，先排查渠道层：

- Webhook 配置看着存在，但没有任何入站
- 出站一直 retry 或 failed
- 长连接渠道时好时坏
- 外部客户端点不开文件

## 文件为什么在渠道里常变成下载链接

因为渠道客户端通常无法直接理解 Wunder 内部的 `/workspaces/...` 路径。

所以出站前会把这些路径改写到：

- `/wunder/temp_dir/download`

如果这条链路坏了，渠道看起来就像“消息发出去了，但附件打不开”。

## 实施建议

- 渠道问题经常发生在账号、长连接、outbox 和下载改写，不一定是模型问题。
- 管理员侧看账号、绑定和渠道会话；用户侧看 runtime logs。
- 外部附件打不开时，优先检查 `temp_dir` 下载链路和改写逻辑。

## 延伸阅读

- [渠道 Webhook](/docs/zh-CN/integration/channel-webhook/)
- [临时目录与文档转换](/docs/zh-CN/integration/temp-dir/)
- [管理端界面](/docs/zh-CN/surfaces/web-admin/)
