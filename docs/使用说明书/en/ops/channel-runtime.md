---
title: Channel Runtime
summary: Channel issues often stem not from the model, but from the access chain: accounts, persistent connections, webhooks, outbox, and download link rewriting.
read_when:
  - You are troubleshooting channel anomalies for Feishu, WeChat, QQBot, WhatsApp, XMPP, etc.
  - You want to know what channel runtime interfaces are available on both admin and user sides
source_docs:
  - docs/API文档.md
  - docs/设计方案.md
  - docs/系统介绍.md
---

# Channel Runtime

Many channel issues appear as "the model didn't respond," but actually fail much earlier at the access layer.

## Key Points on This Page

This page only covers what to check first for channel operations:

- Is the account actually online?
- Is the persistent connection alive?
- Is inbound traffic coming in?
- Is outbound traffic stuck in the outbox?

## Most Important Admin Endpoints

- `GET /wunder/admin/channels/accounts`
- `POST /wunder/admin/channels/accounts/batch`
- `DELETE /wunder/admin/channels/accounts/{channel}/{account_id}`
- `GET /wunder/admin/channels/accounts/{channel}/{account_id}/impact`
- `GET /wunder/admin/channels/bindings`
- `GET /wunder/admin/channels/user_bindings`
- `GET /wunder/admin/channels/sessions`

These endpoints help answer:

- Which accounts are abnormal?
- Which bindings will be affected?
- Which `user_id` / `agent_id` a channel session is actually bound to?

## User-Side Runtime Log Endpoints

- `GET /wunder/channels/runtime_logs`
- `POST /wunder/channels/runtime_logs/probe`

These endpoints help answer:

- Have there been recent persistent connection failures, reconnects, or runtime warnings for accounts visible to the current user?
- Is the log panel blocked by permissions or filter conditions?

## When Not to Blame the Model First

If you see these symptoms, check the channel layer first:

- Webhook configuration exists, but no inbound traffic
- Outbound keeps retrying or failing
- Persistent connection channels are intermittently working
- External clients cannot open files

## Why Files Often Become Download Links in Channels

Because channel clients typically cannot directly understand Wunder's internal `/workspaces/...` paths.

So before outbound, these paths are rewritten to:

- `/wunder/temp_dir/download`

If this link breaks, it looks like "the message was sent, but the attachment won't open."

## Implementation Recommendations

- Channel issues often occur in accounts, persistent connections, outbox, and download rewriting—not necessarily model issues.
- Admin side: check accounts, bindings, and channel sessions; user side: check runtime logs.
- When external attachments won't open, prioritize checking the `temp_dir` download path and rewriting logic.

## Further Reading

- [Channel Webhook](/docs/en/integration/channel-webhook/)
- [Temp Directory and Document Conversion](/docs/en/integration/temp-dir/)
- [Admin Interface](/docs/en/surfaces/web-admin/)