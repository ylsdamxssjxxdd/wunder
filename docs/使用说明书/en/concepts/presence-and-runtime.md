---
title: Runtime and Online Status
summary: A conversation is not just static history: it also carries runtime state such as running, waiting for approval, or waiting for input, and that state tells you what the session is doing right now.
updated_at: 2026-10-13
---

A conversation is more than a chat log. It also carries **runtime state** that tells you what the session is doing right now.

## Two layers of state

The first layer is what the session is doing at this moment.

| State | Meaning |
| --- | --- |
| Running | Actively working |
| Waiting for approval | Paused on an action that needs your go-ahead |
| Waiting for your input | The agent is waiting for you to add information |
| Interrupting | Being interrupted |
| Idle | Nothing to do, waiting for your next message |
| Not loaded | Not yet loaded into memory |

The second layer is a summary snapshot the interface reads directly: whether output is still streaming, whether the session is waiting, whether it is loaded, whether it is active, and whether it has already reached a terminal state.

## Where it shows up in the interface

- **Session list**: the status badges come straight from runtime state — a green dot means an active session, a spinning icon means running, and a waiting icon means waiting for approval or input.
- **Session detail**: the status indicator tells you which stage you are at, whether you can send a new message, and whether any approval is pending.

## Judging a stuck session

Runtime state is the starting point for troubleshooting.

| You want to know | Look at |
| --- | --- |
| Whether it is still running | Whether output is still streaming |
| Whether it is waiting | Whether it is in a waiting state |
| Whether it has finished | Whether it has reached a terminal state |
| Whether it can still be observed | Whether it is loaded and active |

Do not rely on a single coarse running / not-running flag. It cannot tell "still streaming" apart from "waiting for approval", and it says nothing about whether the session is loaded or has reached a terminal state. Read the full set of snapshot fields instead.

## When state looks inconsistent

If the interface and the actual state do not match:

1. Wait a few seconds to see whether it recovers on its own.
2. Refresh the page or restart the app.
3. Use "Reset working state".

## Common mistakes

- Waiting for approval is a **pause**, not an ending — it is still within the session lifecycle.
- State belongs on the list and event pages too, not only the detail page.
- The event stream carries the timeline while the runtime snapshot carries the present moment; they are not the same layer.

## Further reading

- [Session states](/docs/en/capabilities/stream-events/) — what each state means in detail
- [The agent loop](/docs/en/concepts/core-agent-loop/) — how state changes
- [Streaming execution](/docs/en/concepts/streaming/) — how state is pushed
