---
title: Channel Tool
summary: The channel tool has been removed from wunder; kept only as a historical reference.
updated_at: 2026-10-13
---

> **Removed**: This tool has been removed from wunder and is not visible to the agent or users; kept only as a historical reference.

It once had two actions:

- `list_contacts`: list contacts on an external channel, filterable by keyword; the result carried a total, paging information, and the source scope.
- `send_message`: send a channel message to a contact.

On sending, `wait` decided which receipt you got:

- `wait=true`: closer to a synchronous "delivered" result
- `wait=false`: closer to "queued in the outbox", returning only a queued status
