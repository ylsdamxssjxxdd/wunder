---
title: Channels
summary: The support level of Feishu, QQ, WeChat, XMPP, and generic webhooks; the binding score that routes a message to its agent and workspace; inbound, outbound, and rate-limit parameters.
updated_at: 2026-10-16
---

## Scope of responsibility

All execution happens in the engine. A channel does just two things: parse an external message into a unified inbound shape, and deliver the unified outbound shape back into an external protocol. Protocol adaptation stops at the entry; a channel does not copy business logic, does not keep its own storage, and does not change thread semantics. The same task thread can take a message from Beehive, continue it from Feishu, and finish it from Pupa; a channel never creates a second execution path.

A form (Pupa / Beehive / Helm / Hull) is the shape of running and access; a channel (Feishu / QQ / WeChat …) is an off-site platform. The two are not one layer. The statement that many entries share one engine holds; what tends to be written wrong is the support level.

## Support level

- **Feishu (feishu)**: a persistent connection plus a webhook, fully implemented, with a dedicated webhook entry point.
- **QQ Bot (qqbot)**: a persistent connection plus a webhook, fully implemented, with a dedicated webhook entry point.
- **WeChat (weixin)**: runs on a persistent connection (iLink long polling), fully implemented, and the largest in scale.
- **XMPP (xmpp)**: a persistent connection plus a webhook. It is disabled by default and is only a placeholder then, returning a "not enabled" notice for all send and receive; it becomes a real implementation only once explicitly turned on.
- **WeCom (wechat)**: webhook only, partially implemented (send, receive, and signature verification), with no persistent connection.
- **WeChat Official Account (wechat_mp)**: webhook only, partially implemented, with no persistent connection.
- **WhatsApp Cloud (whatsapp_cloud)**: webhook only, partially implemented, with no persistent connection.

Only four channels use a persistent connection: Feishu, QQ Bot, XMPP, and WeChat (weixin). WeCom, the Official Account, and WhatsApp Cloud are pure webhook callbacks.

The following platforms have no adapter at all: telegram, discord, slack, LINE, DingTalk. They exist only as generic-webhook directory entries, handled through the generic entry point and generic parsing — with no platform-specific signature verification, rich-text mapping, attachment download, or proactive push. The accurate statement is that they are visible in the directory and can send and receive text over a generic webhook, but they have no platform-specific adapter. Writing them up as "supported channels" is wrong.

## Binding resolution

An inbound message is first resolved by binding: candidates are scored by specificity, the single best one is taken, and priority breaks a tie.

| Dimension | Score | Notes |
| --- | --- | --- |
| Exact peer_id match | 8 | Supports the * wildcard, and a wildcard scores nothing |
| peer_kind match | 4 | dm/direct/single/user are treated as the same kind |
| account_id match | 2 | Empty means wildcard |
| channel match | 1 | Empty means wildcard |

A hit yields the target agent and any tool overrides, after which the workspace is decided: empty, as well as the default and __default__ aliases, all point to that user's default agent workspace. In the Hull form, workspaces are flattened per user into a single cloud directory, so files arriving from different channels for the same user land in the same place.

## Inbound, outbound, and rate limiting

Inbound uses a bounded queue: capacity 512, concurrency 8, enqueue timeout 80 ms. A full queue or an enqueue timeout counts as backpressure and reports "queue busy" rather than piling up without limit. The visible effect is that the platform redelivers or briefly delays; messages are not silently dropped.

Outbound runs on a persistent queue with exponential backoff, defaulting to a 500 ms polling interval, batches of 50, and at most 5 retries, with a backoff base of 2 s and a ceiling of 60 s.

Outbound delivery has a switch. When it is off, delivery is instead attempted once synchronously at enqueue time and is no longer redelivered by the polling loop. When troubleshooting a "received but no reply" case, check this switch first: once it is off and that one synchronous attempt fails, the message sits in the outbound queue waiting for manual handling.

Rate limiting is a token bucket of QPS plus a concurrency cap per channel account; both at 0 means no limiting. A rate-limited request does not consume a concurrency slot and is handed to the outbound queue for backoff redelivery.

## Attachments and media

Inbound media is downloaded, transcoded, and dropped into the workspace for that channel before being handed to the engine; platform-specific files have their own download entry points and likewise write into the workspace; and a further kind of pending file is reserved first and fetched on demand. Outbound attachments are extracted from the body, from Markdown, or from a workspace path and then uploaded. Channel clients usually do not understand internal paths, so attachments go out as an upload link rather than a path string thrown at the user.

## Webhook entry points and verification

The generic entry point is `POST /wunder/channel/{provider}/webhook`; Feishu and QQ Bot have their own dedicated entry points at `POST /wunder/channel/feishu/webhook` and `POST /wunder/channel/qqbot/webhook`.

Verification differs by platform: some loop back a verification token from configuration, some use a content signature, and WeCom additionally verifies the signature and decrypts the encrypted message. The generic webhook does no platform-specific verification, so platforms that use the generic entry point need to close this off themselves at a gateway or reverse proxy to guarantee a trustworthy source.

## Troubleshooting messages

1. **Presence**: for a persistent-connection channel, check whether its connection process is running; a webhook-only channel has no notion of presence, so check only whether the callback arrives.
2. **Whether the callback arrives**: check the platform-side push logs and the channel's runtime logs.
3. **Whether it was routed**: check the binding score and the enabled state; a binding to a different agent looks exactly like no response at all.
4. **Arrived but no reply**: check for records stuck in the outbound queue, whether the outbound delivery loop is off, and whether rate limiting was hit.
5. **XMPP specifics**: confirm whether it is enabled — while disabled by default it is only a placeholder, and errors say so directly.

## Points not to mix up

- telegram / discord / slack / LINE / DingTalk have no adapter; they are only generic directory entries.
- WeCom and the Official Account have no persistent connection; only Feishu, QQ Bot, XMPP, and WeChat have one.
- XMPP is disabled by default, and while disabled it is a placeholder.
- A channel cannot be configured with business logic; it has only two layers, the target agent and tool overrides, and everything else lives in the engine and on the management side.
- The core capabilities are the same across channels; the differences are only in protocol expression (rich text, attachments, cards) and whether a persistent connection is supported.
- The inbound queue does not guarantee no loss by queuing forever; it is bounded at 512 with an 80 ms enqueue timeout, and anything beyond goes to backpressure and platform redelivery.
- A failed outbound delivery is not dropped outright; it uses a persistent queue with exponential backoff and at most 5 retries.
- Pupa and Helm do not go through the channel layer: Pupa is a native in-process call, and Helm uses its own access path.
