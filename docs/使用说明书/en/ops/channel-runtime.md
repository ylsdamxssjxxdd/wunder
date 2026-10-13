---
title: Channel Runtime
summary: Connecting and running external channels such as Feishu and WeChat day to day. Channel problems often lie in the access layer rather than the model.
updated_at: 2026-10-13
---

Channels are the bridge between wunder and external platforms. Once connected, users can talk to the agent directly from tools they already know, such as Feishu, WeCom, and Telegram, and every channel's connection and runtime state is managed from one place in Bridge. When a channel acts up, check the access layer before the model: the great majority of failures come from accounts, callback addresses, or the inbound and outbound paths rather than from the agent itself.

## How Channels Work

Channel adapters come in two kinds: some platforms use a dedicated adapter, and others use a generic webhook.

### Message Flow

```
External platforms (Feishu / WeChat / QQ / WhatsApp / XMPP / Telegram / Discord / Slack / LINE / DingTalk, etc.)
        ↓ Webhook / persistent connection / generic webhook routing
    Unified access layer
        ↓
    Inbound queue (fast intake)
        ↓
    Dispatch execution (message processing)
        ↓
    Outbound queue (reply delivery)
        ↓
    Channel adapter (send to platform)
```

### Key Components

| Component | Role |
| --- | --- |
| **Channel account** | An identity bound to an external platform |
| **Binding** | The mapping between a channel user and a system user |
| **Inbound path** | External messages entering the system |
| **Outbound path** | System replies sent out to the outside |
| **Runtime logs** | A record of channel runtime state |

## Adding a New Channel

1. **Create an app on the external platform**
   - For example a Feishu custom app or a WeCom app
   - Obtain the credentials such as the app ID and app secret

2. **Add the channel account in Bridge**
   - Bridge → Channel Management → Add account
   - Fill in the channel type and credential details

3. **Configure the webhook**
   - Set the message receiving address
   - Configure the webhook URL on the external platform
   - Make sure the address is reachable from outside

4. **Test the connection**
   - Send a test message from the external platform
   - Check that the reply arrives
   - Confirm in the runtime logs

5. **Bind users**
   - Bind channel users to system users
   - Confirm messages route correctly

## Monitoring Channel Health

### Bridge

- Channel account status (online or offline)
- Message send and receive statistics
- Abnormal-activity alerts

### Beehive

- The runtime log panel
- Connection status
- Recent activity

## Troubleshooting Channel Problems

Work down through the layers in this order:

**1. Account online status**
- Check the account status in Bridge
- If it is offline, check the credentials and the network

**2. Callback reachability**
- Check that the webhook address is reachable
- Look at the platform's message push logs
- Confirm the callback address has not changed

**3. Inbound intake**
- Confirm the external message has entered the system
- Check the inbound logs

**4. Outbound delivery**
- Check whether replies are piling up in the queue
- Check the outbound queue state
- Look for failed sends

**5. File attachments**
- Confirm the channel client can open the attachment link
- Check that the temporary-directory download service is working

## Common Channel Problems

| Symptom | Likely cause | Where to look |
| --- | --- | --- |
| Messages don't arrive | Webhook not connected | Check the address and configuration |
| Messages can't be sent | Outbound queue backlog | Check the outbound queue |
| Attachments won't open | Broken download link | Check the temporary directory |
| Connection drops in and out | Network or credential problem | Check the account and the network |
| Duplicate replies | Duplicate message delivery | Check idempotent handling |

## Routine Channel Maintenance

### Regular Checks

- Account online status
- Whether messages send and receive normally
- Whether there is any backlog or failure

### Account Management

- Manage channel accounts in bulk
- Review the impact scope of a binding
- Clean up invalid bindings

### Log Management

- Read the runtime logs to understand anomalies
- Archive old logs regularly
- Watch for alerts and reconnect records

## Further Reading

- [Channels (Core Concept)](/docs/en/concepts/core-channels/)
- [Stability](/docs/en/concepts/core-stability/) — error recovery mechanisms
- [Admin Panels Index](/docs/en/reference/admin-panels/) — where the management entry points are
