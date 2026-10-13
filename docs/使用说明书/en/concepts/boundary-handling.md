---
title: Boundary Handling
summary: Context overflow, network interruption, tool failure, queued tasks — the system stays stable under these boundary conditions instead of crashing outright.
updated_at: 2026-10-13
---

Conversations grow longer, the network jitters, tools error out, and several requests arrive at once; under these conditions wunder aims to hold steady rather than collapse as a whole.

## The three handling principles

- **Contain the blast radius**: when one link fails, it does not drag the others down with it.
- **Stay recoverable**: failure is not the end; the system keeps a recovery path.
- **Stay reviewable**: problems of the same kind can be categorized, reproduced, and verified.

## Context overflow: graduated compression

As a conversation grows longer and tool results pile up, the total token count can exceed the model's ceiling. The system compresses progressively in four levels:

| Level | What it does | Loss |
| --- | --- | --- |
| Level 1 | Remove intermediate tool results | Very low |
| Level 2 | Summarize the conversation history | Medium |
| Level 3 | Remove earlier parts of the conversation | Higher |
| Level 4 | Emergency degradation | Very high |

**The system prompt never takes part in compression and stays frozen**, so compression never changes the agent's behavioral setup.

## A dropped connection: automatic reconnect plus event replay

A connection can drop because of network jitter. After a disconnect the interface shows a prompt first, then reconnects automatically with exponential backoff (1 s → 2 s → 4 s → 8 s → 16 s, up to 32 s). Events that were not pushed during the disconnect are cached and replayed once the reconnect succeeds; if the replay also fails, the interface prompts for a manual refresh.

## Tool failure: handled by type

| Failure type | Handling |
| --- | --- |
| Wrong parameters | The agent corrects the parameters itself and retries |
| Permission denied | Execution stops and waits for your approval |
| Temporary error (file locked, network jitter) | Automatic retry |
| System error | Limited retry plus degradation |

## Oversized tool output: three layers of containment

| Layer | Role |
| --- | --- |
| Execution budget | Caps the size of a single output |
| Result trimming | Keeps the head and tail when the limit is exceeded |
| Pre-model check | Trims once more before the input reaches the model |

A trimmed result tells the agent explicitly that it was cut, so the agent can switch strategy.

## Several requests at once: queueing plus priority

When a conversation is busy, new requests are not turned away but queued and run in turn once it is free. Priority runs from high to low: administrators > high priority > normal > low priority.

## What you can do

- **Long conversations**: watch context usage and compress proactively with `/compact` when needed; start a new session once a topic is done.
- **Network jitter**: when the interface shows "connection lost", wait for it to reconnect automatically instead of refreshing repeatedly; if the reconnect fails, refresh manually as prompted.
- **Tool failure**: read the failure notice to find the cause and adjust as it suggests (for example, give the correct file path); if it keeps failing, ask an administrator to check the system logs.
- **System exceptions**: if there is no response for a long time, try cancelling or resetting first; if the state is clearly wrong, use "reset working state"; if it keeps happening, ask an administrator to troubleshoot.

## Easy to get wrong

- An agent answering does not mean everything is fine; look at whether it stays stable over time and whether it catches failures when they happen.
- Retrying is not a cure-all; different error types take different handling.
- Boundary cases are not rare; they show up often in long conversations and complex tasks.

## Further reading

- [Stability](/docs/en/concepts/core-stability/) — stability design principles
- [Context Compression](/docs/en/concepts/core-context-compression/) — the detailed compression mechanism
- [Troubleshooting](/docs/en/help/troubleshooting/) — solving specific problems
