---
title: 微信 iLink 渠道
summary: `weixin` 是 Wunder 的新微信渠道（iLink 协议），与 `wechat` / `wechat_mp` 不同；本文给出选型、扫码接入、运行态检查与故障定位。
read_when:
  - 你要接入新微信渠道并替换旧的 `wechat` / `wechat_mp`
  - 你想确认用户是否可以直接扫系统生成二维码
  - 你在排查 `context_token` 缺失、会话过期、文件下载失败
source_docs:
  - docs/方案/新微信渠道落地方案.md
  - docs/API文档.md
  - src/channels/weixin.rs
  - src/channels/service.rs
  - src/api/user_channels.rs
---

# 微信 iLink 渠道

`weixin` 是独立 provider，不是旧微信渠道的参数变体。  
在 Wunder 中请按以下规则选型，避免接错。

## 30 秒选型

| 场景 | 选择 |
| --- | --- |
| 企业微信应用回调 | `wechat`（旧） |
| 微信公众号回调 | `wechat_mp`（旧） |
| iLink 协议微信能力（openclaw-weixin） | `weixin`（新） |

## 关键差异（新旧区分）

| 维度 | `weixin`（新） | `wechat` / `wechat_mp`（旧） |
| --- | --- | --- |
| 入站模型 | 长轮询 `ilink/bot/getupdates` | Webhook 回调 |
| 回复关键字段 | 必须带 `context_token` | 不依赖 `context_token` |
| 登录方式 | 扫码获得 `bot_token` | 传统 app/corp 参数 |
| 媒体链路 | CDN + AES 解密/加密 | 平台原生接口 |
| 推荐状态 | 新接入优先 | 维护态/兼容存量 |

## 用户侧扫码接入（已支持）

用户在渠道设置选择 `weixin` 后，可直接执行：

1. 点击“生成二维码”
2. 用微信扫码
3. 点击“等待确认”
4. 系统自动回填 `bot_token`、`ilink_bot_id`、`ilink_user_id`、`api_base`
5. 保存账号配置

说明：
- 回填成功后必须保存，长轮询 worker 才会真正生效。
- 二维码会话有 TTL，过期需重新生成。

## 最小可用配置（P0）

```json
{
  "weixin": {
    "api_base": "https://ilinkai.weixin.qq.com",
    "bot_token": "<required>",
    "ilink_bot_id": "<required>",
    "long_connection_enabled": true
  }
}
```

可选增强项：
- `cdn_base`
- `bot_type`
- `allow_from`
- `poll_timeout_ms` / `api_timeout_ms`
- `max_consecutive_failures` / `backoff_ms`
- `route_tag`

## 文件与媒体链路

当前实现已支持：
- 出站附件：先 `getuploadurl`，再 CDN 上传，最后 `sendmessage` 引用媒体
- 入站附件：解析 `item_list` 媒体项，下载 CDN 对象并解密后落地到工作区

排查优先级：
1. `media_enabled` 是否开启
2. `cdn_base` 是否可达
3. AES key 是否正确（base64/raw16 或 hex32）
4. 附件是否超出大小限制

## 常见问题定位

### 报错 `weixin outbound context_token missing`

原因：回复消息没拿到上一条入站的 `context_token`。  
处理：确认当前回复链路是否基于同一会话，且入站消息元数据已保留 `weixin_context_token`。

### 长时间收不到消息

检查：
- 账号状态是否 `active`
- `long_connection_enabled` 是否开启
- `bot_token` / `ilink_bot_id` 是否有效
- 管理端运行态是否出现 `long_connection_session_expired`

### 会话频繁过期（session expired）

通常是登录态失效，建议重新扫码，获取新的 token 并保存。

## 对接建议（迁移旧微信渠道时）

1. 并行运行：先保留旧渠道，新增流量优先 `weixin`
2. 分批切换：按账号逐步迁移
3. 观察指标：入站成功率、出站成功率、首包时延、失败 TopN
4. 稳定后再把旧渠道标记为维护态

## 延伸阅读

- [渠道 Webhook](/docs/zh-CN/integration/channel-webhook/)
- [API 索引](/docs/zh-CN/reference/api-index/)
- [管理员界面](/docs/zh-CN/surfaces/web-admin/)
