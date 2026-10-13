---
title: 渠道 Webhook
summary: 外部渠道发来的消息（飞书、微信、QQ、XMPP 等）统一从一个 Webhook 地址进来，再交给后台异步处理。
---

# 渠道 Webhook

把飞书、微信、QQ、XMPP 这类外部渠道接进 wunder，入站消息统一走一个地址：

- `/wunder/channel/{provider}/webhook`

## Webhook 与长连接

有的渠道主要靠 Webhook 推送事件，有的渠道还支持长连接补充（飞书、QQBot、XMPP 都维护了长连接运行态）。所以接一个渠道前，先确认它走哪种方式，别以为接渠道就是配一个地址这么简单。

## 最短链路：四步接通

1. 渠道的请求先进 Webhook；
2. 适配器校验签名，把消息统一成 wunder 的格式；
3. 快速回执后，消息进入后台队列异步执行；
4. 执行结果通过出站缓冲投递回渠道。

回执要快：Webhook 不能卡在模型推理上，出站失败也不该反过来拖死入站。

## 现成支持哪些渠道

已经明确的入站地址：

- `/wunder/channel/feishu/webhook`
- `/wunder/channel/wechat/webhook`
- `/wunder/channel/wechat_mp/webhook`
- `/wunder/channel/qqbot/webhook`
- `/wunder/channel/whatsapp/webhook`
- `/wunder/channel/xmpp/webhook`

也保留统一的 `/wunder/channel/{provider}/webhook`，按注册的渠道分发。

## 关键设置与常见坑

- 出站有独立的缓冲层，负责异步投递、失败重试和状态记录，回退地址也在这里兜底——所以渠道链路是可恢复的，不会一次失败就丢掉。
- 渠道客户端看不懂 wunder 内部的工作区路径。发出去的正文和附件里，内部路径会被自动改写成可点击的临时下载链接，外部才点得开。
- 常见问题按这个顺序看：账号配了但 Webhook 没通；验签失败；出站一直重试；配了长连接但后台进程没起来；文件链接没改写导致外部打不开。
- 排查时先去舰桥看渠道监控、账号运行态、入站/出站统计，比只盯模型日志有效——很多问题其实出在接入层。

## 延伸阅读

- [舰桥界面](/docs/zh-CN/surfaces/web-admin/)
- [认证与安全](/docs/zh-CN/ops/auth-and-security/)
- [微信 iLink 渠道（新微信接入）](/docs/zh-CN/integration/weixin-channel/)
- [API 索引](/docs/zh-CN/reference/api-index/)
