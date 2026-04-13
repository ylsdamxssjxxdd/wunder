---
title: Channel Webhook
summary: Wunder uses /wunder/channel/{provider}/webhook as a unified inbound entry for external channels, then hands off to ChannelHub and the outbox for processing.
read_when:
  - You want to connect Feishu, WeCom, QQBot, XMPP, and other channels into Wunder
  - You want to understand the relationship between Webhook, persistent connections, and the outbox
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
  - docs/设计方案.md
  - src/api/channel.rs
  - src/channels/catalog.rs
---

# Channel Webhook

Channel integration in Wunder is not a "peripheral feature" but one of the official entry points.

Its unified entry is:

- `/wunder/channel/{provider}/webhook`

## Core Goals of This Pipeline

Unified handling of:

- Signature verification
- Channel message normalization
- Fast acknowledgment (ACK)
- Background async scheduling
- Outbound delivery and retry

So the Webhook is just the entry point, not the entire logic.

## Existing Typical Entries

The system currently supports these typical channel entries:

- `/wunder/channel/feishu/webhook`
- `/wunder/channel/wechat/webhook`
- `/wunder/channel/wechat_mp/webhook`
- `/wunder/channel/qqbot/webhook`
- `/wunder/channel/whatsapp/webhook`
- `/wunder/channel/xmpp/webhook`

Also preserved:

- `/wunder/channel/{provider}/webhook`

For dispatch through the unified adapter registry.

## Why a Unified Entry

If every channel had its own inbound pipeline, things would quickly spiral out of control:

- Signature verification logic scattered
- Inconsistent outbound retry behavior
- Inconsistent monitoring and logging standards
- Increasing cost to onboard new channels

That is why Wunder currently uses `ChannelAdapterRegistry` to uniformly assemble adapters.

## What Happens After a Webhook Comes In

It is best understood as four steps:

1. The channel request enters the Webhook
2. The adapter verifies the signature and normalizes the message
3. The message is quickly ACKed and enters the background queue
4. Execution results are delivered outbound through the outbox

This means:

- The Webhook should not block for a long time on model inference
- Outbound failures should not in turn choke inbound processing

## Are Persistent Connections and Webhooks Opposed

No.

Some channels are primarily Webhook-based; others support persistent connections as a supplement.

For example, the system already maintains running persistent connections for:

- Feishu persistent connection
- QQBot persistent connection
- XMPP persistent connection

So you should not think of "channel integration" as simply having a single Webhook URL.

## What Does the Outbox Do

The outbox is the outbound buffer layer.

It is responsible for:

- Async delivery
- Retries
- Failure status recording
- Compatibility with official adapters and fallback URLs

This turns the channel pipeline from "synchronous request / synchronous response" into a truly recoverable async system.

## Why Files Get Rewritten to Download Links

Many channel clients cannot directly understand Wunder's internal workspace paths.

So the system rewrites the following in message bodies and attachments:

- `/workspaces/...`

into:

- `/wunder/temp_dir/download?...`

This way external channel clients can actually open them.

## What to Look at in the Admin Panel

If you are troubleshooting channel issues, the admin side should first check:

- Channel monitoring page
- Account running status
- Inbound/outbound statistics
- Runtime logs

This is more effective than staring only at model logs, because many issues actually occur at the integration layer.

## Most Common Problems

- Channel account configuration exists, but the webhook is not connected
- Signature verification failure
- Outbox keeps retrying
- Persistent connection configured but the worker is not running
- File links not rewritten, causing external clients to fail to open them

## Further Reading

- [Admin Interface](/docs/en/surfaces/web-admin/)
- [Authentication and Security](/docs/en/ops/auth-and-security/)
- [WeChat iLink Channel (New WeChat Integration)](/docs/en/integration/weixin-channel/)
- [API Index](/docs/en/reference/api-index/)
