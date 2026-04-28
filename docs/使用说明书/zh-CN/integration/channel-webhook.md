---
title: 渠道 Webhook
summary: Wunder 用 `/wunder/channel/{provider}/webhook` 统一承接外部渠道入站，再交给 ChannelHub 和 outbox 处理。
read_when:
  - 你要把飞书、企业微信、QQBot、XMPP 等渠道接进 Wunder
  - 你想知道 Webhook、长连接和 outbox 的关系
source_docs:
  - docs/API文档.md
  - docs/设计文档/01-系统总体设计.md
  - docs/设计文档/07-接入、网关与渠道系统设计.md
  - src/api/channel.rs
  - src/channels/catalog.rs
---

# 渠道 Webhook

渠道接入在 Wunder 里不是“外设功能”，而是正式入口之一。

它的统一入口是：

- `/wunder/channel/{provider}/webhook`

## 这条链路的核心目标

统一处理这些事情：

- 验签
- 渠道消息标准化
- 快速回执
- 后台异步调度
- 出站投递与重试

所以 Webhook 只是入口，不是全部逻辑。

## 已有的典型入口

当前系统里已经明确支持这些典型渠道入口：

- `/wunder/channel/feishu/webhook`
- `/wunder/channel/wechat/webhook`
- `/wunder/channel/wechat_mp/webhook`
- `/wunder/channel/qqbot/webhook`
- `/wunder/channel/whatsapp/webhook`
- `/wunder/channel/xmpp/webhook`

同时也保留：

- `/wunder/channel/{provider}/webhook`

用于走统一注册表分发。

## 为什么要有统一入口

如果每个渠道都各写一套入站主链路，会很快失控：

- 验签逻辑散掉
- 出站重试不一致
- 监控和日志口径不一致
- 新渠道接入成本越来越高

所以 Wunder 当前用 `ChannelAdapterRegistry` 统一装配适配器。

## Webhook 进来之后发生什么

推荐把它理解成四步：

1. 渠道请求进入 Webhook
2. 适配器验签并标准化消息
3. 消息快速 ACK 后进入后台队列
4. 调度执行结果再通过 outbox 投递出去

这意味着：

- Webhook 不应该长时间卡在模型推理上
- 出站失败也不应该反过来拖死入站

## 长连接和 Webhook 是对立关系吗

不是。

有些渠道是 Webhook 为主，有些渠道支持长连接补充。

例如系统里已经明确维护运行态的包括：

- Feishu 长连接
- QQBot 长连接
- XMPP 长连接

所以你不该把“渠道接入”简单理解成只有一个 Webhook URL。

## outbox 是做什么的

outbox 是出站缓冲层。

它负责：

- 异步投递
- 重试
- 失败状态记录
- 兼容官方适配器和回退 URL

这让渠道链路从“同步请求-同步返回”变成真正的可恢复异步系统。

## 文件为什么会被改写成下载链接

很多渠道客户端不能直接理解 Wunder 内部工作区路径。

所以系统会把正文和附件中的：

- `/workspaces/...`

改写成：

- `/wunder/temp_dir/download?...`

这样外部渠道客户端才能真正点开。

## 管理端看什么

如果你在排查渠道问题，管理员侧最应该先看：

- 渠道监控页
- 账号运行态
- 入站/出站统计
- runtime logs

这比只盯着模型日志更有效，因为很多问题根本发生在接入层。

## 最常见的问题

- 渠道账号配置存在，但 webhook 没通
- 验签失败
- outbox 持续重试
- 长连接配置了但 worker 没起来
- 文件链接没改写，导致外部客户端打不开

## 延伸阅读

- [管理端界面](/docs/zh-CN/surfaces/web-admin/)
- [认证与安全](/docs/zh-CN/ops/auth-and-security/)
- [微信 iLink 渠道（新微信接入）](/docs/zh-CN/integration/weixin-channel/)
- [API 索引](/docs/zh-CN/reference/api-index/)
