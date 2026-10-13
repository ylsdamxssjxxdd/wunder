---
title: 渠道
summary: 渠道是入口适配层，不是第二套系统。当前只有 7 个已注册 adapter，长连接只有 4 个；telegram、discord、slack、LINE、DingTalk 没有实现，只是通用 webhook 目录项。
read_when:
  - 要接飞书、微信、QQ、XMPP 或通用 webhook，需要确认真实支持状态
  - 消息进不来或出不去，要定位是入站队列、路由绑定还是出站重试
  - 想知道同一个群里不同用户会落到哪个智能体与工作区
  - 文档或旧材料把某个平台写成「已支持」，需要核对
source_docs:
  - crates/wunder-runtime/src/channels/adapter.rs
  - crates/wunder-runtime/src/channels/registry.rs
  - crates/wunder-runtime/src/channels/service.rs
  - crates/wunder-runtime/src/channels/catalog.rs
  - crates/wunder-runtime/src/channels/binding.rs
  - crates/wunder-runtime/src/channels/inbound_queue.rs
  - crates/wunder-runtime/src/channels/outbox.rs
  - crates/wunder-runtime/src/channels/rate_limit.rs
  - crates/wunder-runtime/src/api/channel.rs
  - crates/wunder-runtime/src/channels/workspace_routing.rs
updated_at: 2026-10-13
---

# 渠道

## 心智模型：渠道只是入口

一切执行都在引擎里，渠道只做两件事：**把外部消息解析成统一入站形态**，**把统一出站形态投回外部协议**。协议适配到入口为止，不复制业务逻辑、不自建存储、不改变线程语义。

因此：同一个任务线程可以从蜂巢发消息、从飞书续聊、用蜂窝收尾；渠道不会创造第二套执行。

> 先分清两个词。**形态**（蜂窝/蜂巢/舵机/舰体）是运行与接入形状，**渠道**（飞书/QQ/微信…）是站外平台。旧文档常把「多入口共用一套引擎」这句话说对，但把渠道清单写错——错在支持范围，不在理念。

## 支持状态（以代码为准）

契约是 `ChannelAdapter { verify_inbound, parse_inbound, send_outbound, health_check }`（`channels/adapter.rs:29-52`）。默认注册表只有 **7 个 adapter**（`channels/registry.rs:63-71`）：

| 渠道 | 接入方式 | 实现状态 |
| --- | --- | --- |
| **飞书 feishu** | 长连接 + webhook | 完整实现，有专用 webhook |
| **QQ Bot qqbot** | 长连接 + webhook | 完整实现，有专用 webhook |
| **微信 weixin** | 长连接（iLink 长轮询） | 完整实现，规模最大 |
| **XMPP xmpp** | 长连接 + webhook | **默认 feature 关闭时是占位 stub**：出入站一律返回 `xmpp feature is disabled`（`channels/xmpp.rs:7-15`）；启用 `xmpp` feature 才是真实现 |
| **企业微信 wechat** | webhook-only | 部分实现（收发与签名校验），无长连接 |
| **公众号 wechat_mp** | webhook-only | 部分实现，无长连接 |
| **WhatsApp Cloud whatsapp_cloud** | webhook-only | 部分实现，无长连接 |

长连接 supervisor **只有 4 个**：`feishu` / `qqbot` / `xmpp` / `weixin`（`channels/service.rs:232-245`）。`wechat`、`wechat_mp`、`whatsapp_cloud` 是纯 webhook 回调。

### 没有实现的平台

`telegram`、`discord`、`slack`、`LINE`、`DingTalk` **没有任何 adapter**。它们只作为 `webhook_mode: "generic"` 的目录项存在于 `channels/catalog.rs:75-122`：走通用 webhook 入口 + 通用解析，不会做平台专属的签名校验、富文本映射、附件下载或主动推送。

**任何文档把这几个写成「已支持渠道」都是错的。** 正确表述：目录可见、可挂通用 webhook 收发文本，不具备平台专属适配。

## 消息怎么找到智能体和工作区

入站消息先做**绑定解析**（`channels/binding.rs:16`）：按特异性打分取唯一最优绑定，分数并列再比 `priority`。

| 维度 | 分值 | 说明 |
| --- | --- | --- |
| `peer_id` 精确匹配 | 8 | 支持 `*` 通配（通配不计分） |
| `peer_kind` 匹配 | 4 | dm/direct/single/user 互相视为同类 |
| `account_id` 匹配 | 2 | 空即通配 |
| `channel` 匹配 | 1 | 空即通配 |

命中后得到 `agent_id` 与 `tool_overrides`，再由 `resolve_channel_workspace_id`（`channels/workspace_routing.rs:5`）决定工作区：缺省与 `default`/`__default__` 别名都指向该用户的默认智能体工作区。舰体形态下工作区按用户摊平为唯一云端目录，所以**同用户不同渠道进来的文件落点一致**。

## 有界与重试（数值都有出处）

### 入站

| 项 | 值 | 出处 |
| --- | --- | --- |
| 入站队列容量 | 512 | `channels/inbound_queue.rs:10` |
| 并发处理 | 8 | `:11` |
| 入队超时 | 80 ms | `:12` |

队列满或入队超时即视为**背压**：用有界队列的通用入队逻辑，失败时报队列忙（`core/bounded_queue.rs`），不无限堆积。表现为平台侧重投或短暂延迟，不是消息被静默丢弃后还要靠人工找回。

### 出站

出站走**持久 outbox + 指数退避**（`channels/outbox.rs:4-27`），默认值：

| 项 | 值 |
| --- | --- |
| 轮询间隔 | 500 ms |
| 单批 | 50 条 |
| 最大重试 | 5 次 |
| 退避基数 / 上限 | 2 s / 60 s（`base × 2^retry`，封顶上限） |

出站 worker 由 `channels.outbox.worker_enabled`（`wunder-core/src/config.rs:2051`）控制：关掉后投递改在入队处同步尝试一次（`channels/service/outbox.rs:50` `try_deliver_outbox_if_worker_disabled`），不再由轮询循环重投。**排查「收到但不回」时先确认这个开关**：关掉 worker 且同步那一次失败，消息就停在 outbox 里等人工处理。

### 限流

按渠道账号维度做**令牌桶 QPS + 并发上限**（`channels/rate_limit.rs:40-90`），由 `ChannelRateLimitConfig` 驱动；两者都为 0 表示不限流。触发限流时不占并发额度，交给 outbox 退避重投。

## 附件与媒体

| 环节 | 位置 | 行为 |
| --- | --- | --- |
| 入站媒体 | `channels/media.rs`（`MediaProcessor::process_inbound`） | 下载、转码、落到该渠道对应工作区，再交给引擎 |
| 平台文件下载 | `channels/feishu_files.rs`、`channels/weixin_files.rs` | 平台专属下载入口，写入工作区 |
| 待取文件 | `channels/pending_files.rs` | 先占位、后按需取 |
| 出站附件 | `channels/outbound_attachments.rs` | 从正文/Markdown/工作区路径抽取附件再上传 |
| 运行日志 | `channels/runtime_log.rs` | 启动与运行状态记录 |

渠道客户端通常不认内部路径，所以附件要走上传-回链，而不是把路径字符串丢给用户。

## Webhook 形态与校验

| 端点 | 说明 |
| --- | --- |
| `POST /wunder/channel/{provider}/webhook` | 通用入口（`api/channel.rs:117`） |
| `POST /wunder/channel/feishu/webhook`、`POST /wunder/channel/qqbot/webhook` | 专用入口（`:107`、`:116`） |

校验方式按平台不同（`api/channel.rs:33-198`）：

- `hub.verify_token` —— 平台配置校验回环。
- `x-hub-signature-256` —— 内容签名。
- 企业微信 `msg_signature` + `encrypt_type` —— 加密消息验签与解密。

通用 webhook 不做平台专属验签，这意味着走 generic 的平台需要自己保证来源可信（网关侧或反向代理侧收口）。

## 排障顺序

1. **在线状态**：长连接渠道看 supervisor 是否在跑（`channels.long_connection.<provider>.supervisor`）；webhook-only 渠道没有「在线」概念，只看回调是否到达。
2. **回调有没有进来**：平台侧推送日志 + 渠道运行日志。
3. **有没有被路由到**：检查绑定打分结果与 `enabled`；绑定了别的智能体看起来就像「没反应」。
4. **进来了没回**：看 outbox 是否有滞留记录、出站 worker 是否被关、限流是否命中。
5. **XMPP 特别项**：确认二进制是否启用了 `xmpp` feature——默认关闭时它是 stub，报错文案直接是 `xmpp feature is disabled`。

## 常见误解

- **「telegram/discord/slack/LINE/DingTalk 已支持」** — 没有 adapter，只是 generic 目录项。
- **「企业微信和公众号有长连接」** — 它们是 webhook-only，长连接只有飞书、QQ Bot、XMPP、微信 weixin 四个。
- **「XMPP 开箱可用」** — 默认 feature 关闭时是占位 stub。
- **「渠道能配置业务逻辑」** — 渠道只有 `agent_id` 与 `tool_overrides` 两层覆盖，其余在引擎与管理侧。
- **「渠道不同能力不同」** — 核心能力同一套，差异在协议表达（富文本、附件、卡片）与是否支持长连接。
- **「入站队列会无限排队保证不丢」** — 队列 512 有界、入队 80 ms 超时，超出走背压由平台重投。
- **「出站失败就丢了」** — 持久 outbox + 指数退避重试，最多 5 次。
- **「蜂窝/舵机也要经过渠道层」** — 不是。蜂窝同进程原生调用，舵机走 CLI 自己的接入，二者不依赖舰体。

## 延伸阅读

- [渠道运行时运维](/docs/zh-CN/ops/channel-runtime/) —— 部署、监控与排障
- [渠道 webhook 接入](/docs/zh-CN/integration/channel-webhook/) —— 回调端点与校验
- [微信渠道接入](/docs/zh-CN/integration/weixin-channel/) —— weixin 长轮询细节
- [工作区路由参考](/docs/zh-CN/reference/workspace-routing/) —— 绑定到工作区的映射规则
- [实时性](/docs/zh-CN/concepts/core-realtime/) —— 事件推送与补水
- [舰桥面板指南](/docs/zh-CN/reference/admin-panels/) —— 渠道账号配置入口
