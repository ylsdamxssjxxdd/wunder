---
title: WeChat iLink Channel
summary: The new WeChat channel uses the iLink protocol and is not the same thing as the legacy Official Account or WeCom channels.
updated_at: 2026-10-13
---

`weixin` is an independent channel, not an old WeChat channel with different parameters.

## Choosing a Channel

| Scenario | Choose |
| --- | --- |
| Enterprise WeChat app callback | `wechat` (legacy) |
| WeChat Official Account callback | `wechat_mp` (legacy) |
| WeChat capability over the iLink protocol | `weixin` (new) |

The main differences between new and old:

- Inbound: `weixin` pulls updates by long polling; the legacy version uses Webhook callbacks.
- Reply requirement: a `weixin` reply must carry the context identifier of the current session; the legacy version does not depend on it.
- Login: `weixin` scans a QR code to obtain the bot token; the legacy version uses traditional enterprise / Official Account parameters.
- Media: `weixin` goes through a CDN and handles encryption and decryption itself; the legacy version goes through the platform's native interface.
- Status: prefer `weixin` for new integrations; the legacy version is in maintenance / compatibility mode for existing deployments.

## Connecting by QR Code

After choosing `weixin` in the channel settings:

1. Click "Generate QR code";
2. Scan it with WeChat;
3. Click "Wait for confirmation";
4. The system fills in the bot token, bot ID, user ID, and interface address automatically;
5. Save the account configuration.

Two notes: after the auto-fill, you **must save** for long polling to actually take effect; the QR code has a validity period and must be regenerated once it expires.

## Key Settings and Common Pitfalls

- Three required fields: interface address, bot token, and bot ID; then turn on the long-connection switch.
- A few optional enhancements remain: CDN address, bot type, allowed caller range, polling and interface timeouts, backoff after consecutive failures, and route tag.
- Media-link troubleshooting order: whether media is enabled, whether the CDN address is reachable, whether the decryption key is correct, and whether the attachment exceeds the size limit.
- A reply reporting "missing session context identifier" means the reply did not receive the context identifier from the previous inbound message. Confirm the reply goes through the same session and that the inbound message preserved it.
- Not receiving messages for a long time: check in order whether the account is active, whether the long connection is on, whether the token and bot ID are valid, and whether the session has expired in the Bridge runtime view.
- Sessions expiring frequently: usually the login state is lost; scan again for a new token and save.

## Migrating from the Old Channel

1. Run in parallel first: keep the old channel and let new traffic prefer `weixin`;
2. Switch over by account, in batches;
3. Watch inbound success rate, outbound success rate, first-packet latency, and the failure ranking;
4. Once stable, mark the old channel as maintenance mode.

## Further Reading

- [API Index](/docs/en/capabilities/api-index/)
- [Admin Interface](/docs/en/surfaces/web-admin/)
