---
title: Channel Webhook
summary: Inbound messages from external channels (Feishu, WeChat, QQ, XMPP, and others) all arrive through a single Webhook address and are then handled asynchronously in the background.
updated_at: 2026-10-13
---

External channels such as Feishu, WeChat, QQ, and XMPP deliver inbound messages through one address:

- `/wunder/channel/{provider}/webhook`

## Webhook vs. Persistent Connection

Some channels push events mainly over a Webhook, while others also support a persistent connection as a supplement (Feishu, QQBot, and XMPP all keep persistent connections running). Connecting a channel therefore involves more than configuring one address.

## Four Steps to Connect

1. The channel's request first enters the Webhook.
2. The adapter verifies the signature and normalizes the message into wunder's format.
3. After a quick acknowledgment, the message enters a background queue and runs asynchronously.
4. The result is delivered back to the channel through the outbound buffer.

The Webhook must acknowledge quickly: it must not stall on model inference, and an outbound failure should not in turn choke inbound processing.

## Channels Supported Out of the Box

The inbound addresses already in place:

- `/wunder/channel/feishu/webhook`
- `/wunder/channel/wechat/webhook`
- `/wunder/channel/wechat_mp/webhook`
- `/wunder/channel/qqbot/webhook`
- `/wunder/channel/whatsapp/webhook`
- `/wunder/channel/xmpp/webhook`

A unified `/wunder/channel/{provider}/webhook` is kept as well, dispatching by registered provider.

## Key Settings and Common Pitfalls

- Outbound traffic has its own buffer layer, handling asynchronous delivery, retries on failure, and status recording, with fallback addresses also covered here. The channel path is therefore recoverable, and a single failure does not lose the message.
- Channel clients cannot make sense of wunder's internal workspace paths. In outgoing bodies and attachments, internal paths are rewritten into clickable temporary download links so external clients can open them.
- Check common problems in this order: the account is configured but the Webhook is not reachable; signature verification fails; outbound keeps retrying; a persistent connection is configured but the background process is not running; file links are not rewritten so external clients cannot open them.
- When troubleshooting, start from the admin panel's channel monitoring, account running status, and inbound/outbound statistics rather than from model logs alone, because many problems lie in the integration layer.

## Further Reading

- [Admin Interface](/docs/en/surfaces/web-admin/)
- [Authentication and Security](/docs/en/ops/auth-and-security/)
- [WeChat iLink Channel (New WeChat Integration)](/docs/en/integration/weixin-channel/)
- [API Index](/docs/en/reference/api-index/)
