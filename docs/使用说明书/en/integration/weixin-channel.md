---
title: WeChat iLink Channel
summary: "`weixin` is Wunder's new WeChat channel (iLink protocol), different from `wechat` / `wechat_mp`; this article covers selection, QR code integration, runtime checks, and troubleshooting."
read_when:
  - "You need to integrate the new WeChat channel and replace old `wechat` / `wechat_mp`"
  - "You want to confirm if users can directly scan system-generated QR codes"
  - "You are troubleshooting `context_token` missing, session expired, or file download failures"
source_docs:
  - docs/方案/新微信渠道落地方案.md
  - docs/API文档.md
  - src/channels/weixin.rs
  - src/channels/service.rs
  - src/api/user_channels.rs
---

# WeChat iLink Channel

`weixin` is an independent provider, not a parameter variant of old WeChat channels.  
In Wunder, please follow the rules below for selection to avoid incorrect integration.

## 30-Second Selection Guide

| Scenario | Choice |
| --- | --- |
| Enterprise WeChat app callback | `wechat` (legacy) |
| WeChat Official Account callback | `wechat_mp` (legacy) |
| iLink protocol WeChat capability (openclaw-weixin) | `weixin` (new) |

## Key Differences (New vs Old)

| Dimension | `weixin` (new) | `wechat` / `wechat_mp` (legacy) |
| --- | --- | --- |
| Inbound model | Long polling `ilink/bot/getupdates` | Webhook callback |
| Reply key field | Must include `context_token` | No `context_token` dependency |
| Login method | Scan QR code to get `bot_token` | Traditional app/corp parameters |
| Media link | CDN + AES decrypt/encrypt | Platform native API |
| Recommended status | Preferred for new integrations | Maintenance mode/legacy support |

## User-Side QR Code Integration (Supported)

After selecting `weixin` in channel settings, users can directly:

1. Click "Generate QR Code"
2. Scan with WeChat
3. Click "Wait for Confirmation"
4. System auto-fills `bot_token`, `ilink_bot_id`, `ilink_user_id`, `api_base`
5. Save account configuration

Notes:
- After successful auto-fill, you must save for the long polling worker to take effect.
- QR code session has TTL; regenerate if expired.

## Minimum Viable Configuration (P0)

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

Optional enhancements:
- `cdn_base`
- `bot_type`
- `allow_from`
- `poll_timeout_ms` / `api_timeout_ms`
- `max_consecutive_failures` / `backoff_ms`
- `route_tag`

## File and Media Link

Current implementation supports:
- Outbound attachments: First `getuploadurl`, then CDN upload, finally `sendmessage` references the media
- Inbound attachments: Parse `item_list` media items, download CDN objects and decrypt before saving to workspace

Troubleshooting priority:
1. Whether `media_enabled` is on
2. Whether `cdn_base` is reachable
3. Whether AES key is correct (base64/raw16 or hex32)
4. Whether attachment exceeds size limit

## Common Issue Diagnosis

### Error `weixin outbound context_token missing`

Cause: Reply message didn't get the `context_token` from the previous inbound message.  
Resolution: Confirm current reply chain is based on the same session, and inbound message metadata has preserved `weixin_context_token`.

### Not receiving messages for a long time

Check:
- Whether account status is `active`
- Whether `long_connection_enabled` is on
- Whether `bot_token` / `ilink_bot_id` is valid
- Whether admin console runtime shows `long_connection_session_expired`

### Session frequently expired

Usually login state is lost. Recommend re-scanning to get new token and save.

## Integration Recommendations (When Migrating from Old WeChat Channel)

1. Parallel run: Keep old channel first, new traffic prefers `weixin`
2. Batch switch: Migrate by account gradually
3. Monitor metrics: Inbound success rate, outbound success rate, first packet latency, failure TopN
4. Mark old channel as maintenance mode after stable

## Further Reading

- [Channel Webhook](/docs/en/integration/channel-webhook/)
- [API Index](/docs/en/reference/api-index/)
- [Admin Interface](/docs/en/surfaces/web-admin/)