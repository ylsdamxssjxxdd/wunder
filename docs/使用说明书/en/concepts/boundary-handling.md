---
title: Boundary Handling
summary: Graduated compression on context overflow, reconnect and event replay after a disconnect, tool failure handling by type, and queueing with priority.
updated_at: 2026-10-16
---

Each failure type has its own path: a failure in one link does not drag the others down, every failure keeps a recovery route, and problems of the same kind can be categorized, reproduced, and verified.

## Context Overflow

As a conversation grows longer and tool results pile up, the total token count approaches the model's ceiling. Compression runs progressively in four levels:

| Level | What it does | Loss |
| --- | --- | --- |
| Level 1 | Remove intermediate tool results | Very low |
| Level 2 | Summarize the conversation history | Medium |
| Level 3 | Remove earlier parts of the conversation | Higher |
| Level 4 | Emergency degradation | Very high |

The system prompt never takes part in compression and stays frozen, so compression never changes the agent's behavioral setup.

On long conversations, watch context usage and compress proactively with `/compact` when needed; start a new session once a topic is done.

## Disconnect and Reconnect

After a disconnect caused by network jitter, the interface shows a prompt first and then reconnects automatically with exponential backoff (1 s → 2 s → 4 s → 8 s → 16 s, up to 32 s). Events that were not pushed during the disconnect are cached and replayed once the reconnect succeeds; the interface only prompts for a manual refresh if the replay also fails.

When the interface shows "connection lost", wait for the automatic reconnect instead of refreshing repeatedly.

## Tool Failure

| Failure type | Handling |
| --- | --- |
| Wrong parameters | The agent corrects the parameters itself and retries |
| Permission denied | Execution stops and waits for approval |
| Temporary error (file locked, network jitter) | Automatic retry |
| System error | Limited retry plus degradation |

Read the failure notice for the cause and adjust as it suggests (for example, give the correct file path); if it keeps failing, ask an administrator to check the system logs.

## Oversized Tool Output

Three layers of containment:

| Layer | Role |
| --- | --- |
| Execution budget | Caps the size of a single output |
| Result trimming | Keeps the head and tail when the limit is exceeded |
| Pre-model check | Trims once more before the input reaches the model |

A trimmed result tells the agent explicitly that it was cut, so the agent can switch strategy.

## Queueing and Priority

When a conversation is busy, new requests are not turned away but queued and run in turn once it is free. Priority runs from high to low: administrators > high priority > normal > low priority.

## Limits

- An agent answering does not mean everything is fine; check whether it stays stable over time and whether it reports failures when they happen.
- Retrying applies to some error types only.
- Boundary cases show up often in long conversations and complex tasks.
- If there is no response for a long time, try cancelling or resetting first; if the state is clearly wrong, use "reset working state"; if it keeps happening, ask an administrator to troubleshoot.
