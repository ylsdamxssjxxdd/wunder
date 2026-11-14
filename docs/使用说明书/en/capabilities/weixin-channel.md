---
title: WeChat iLink Channel
summary: Choosing between `weixin` and the older WeChat channels, QR onboarding, required and optional settings, and the order to debug missing messages.
updated_at: 2026-11-14
---

# WeChat iLink Channel

`weixin` is its own channel and its parameters are not interchangeable with the earlier two. Pick by scenario first.

## Choosing

| Scenario | Channel |
| --- | --- |
| WeCom app callback | `wechat` |
| WeChat official-account callback | `wechat_mp` |
| iLink-protocol WeChat capabilities | `weixin` |

The differences: `weixin` long-polls for updates, must echo the conversation identifier on each reply, gets its bot token by QR scan, and handles media through a CDN with its own encryption; the other two use webhook callbacks and platform-native media.

## QR onboarding

1. Pick `weixin` in channel settings and generate a QR code.
2. Scan it with WeChat.
3. Wait for confirmation.
4. The bot token, bot id, user id and endpoint are filled in automatically.
5. **Save the account** — without saving, long polling never starts.

QR codes expire; regenerate if needed.

## Settings

- **Required**: endpoint, bot token, bot id, plus the long-connection switch.
- **Optional**: CDN address, bot type, allowed caller scope, polling and request timeouts, backoff after repeated failures, routing tag.

## Debugging

| Symptom | Check first |
| --- | --- |
| No messages arrive | Account active, long connection on, token and bot id valid, runtime session not expired |
| Reply rejected for a missing conversation identifier | This reply lost the inbound message's identifier; confirm the reply goes to the same conversation and the identifier was kept |
| Media missing | Media enabled, CDN reachable, decryption key correct, attachment under the size limit |
| Session expiring constantly | Usually a stale login: rescan for a new token and save |

## Relationship to other channels

Same engine, same tools and permissions; a channel is just an entry point. Bindings and logs live in Settings → Channels, and the Admin only reflects channel configuration inside preset agent templates.
