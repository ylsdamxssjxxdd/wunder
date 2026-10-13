---
title: Sessions Yield
summary: Letting the agent stop the current turn on purpose, hand control back to the system, and carry on once an external result arrives.
updated_at: 2026-10-13
---

A sessions yield means the current turn stops here; it is not a final reply. It is used when something has already been handed off externally and work resumes once the result comes back.

## When to use it

- Use it when you need to tell the system explicitly to stop this turn and resume later, after an external result.
- Waiting for a command to finish does not need it.

## How to use it

You can include a short note explaining why this turn is stopping:

```json
{
  "message": "The task has been submitted; waiting for an external result"
}
```

## How it differs from a final reply

- A sessions yield gives up control of this turn; when the external result comes back, a new turn starts.
- A final reply means this turn has really finished and hands the conclusion to you.

## Do not use it to wait for commands

The standalone sleep tool has been removed. Waiting for a command to finish does not need it: command execution blocks by default and returns only once there is a result, and to wait for a background command you use the wait window offered by the command session rather than idling.
