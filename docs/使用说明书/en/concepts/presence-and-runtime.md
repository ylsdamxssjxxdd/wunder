---
title: Runtime and Online Status
summary: The six runtime states of a session, interface badges and indicators, how to judge a stuck session, and the sequence for handling inconsistent state.
updated_at: 2026-10-16
---

Alongside its history, a session maintains a set of runtime states — running, waiting for approval, waiting for your input, interrupting, idle, and not loaded — from which the interface derives a summary snapshot.

## Session States

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

## Interface Presentation

- **Session list**: the status badges come straight from runtime state — a green dot means an active session, a spinning icon means running, and a waiting icon means waiting for approval or input.
- **Session detail**: the status indicator tells you which stage you are at, whether you can send a new message, and whether any approval is pending.

## Diagnosing a Stuck Session

Runtime state is the starting point for troubleshooting.

| You want to know | Look at |
| --- | --- |
| Whether it is still running | Whether output is still streaming |
| Whether it is waiting | Whether it is in a waiting state |
| Whether it has finished | Whether it has reached a terminal state |
| Whether it can still be observed | Whether it is loaded and active |

A single coarse running / not-running flag cannot tell "still streaming" apart from "waiting for approval", and it says nothing about whether the session is loaded or has reached a terminal state. Read the full set of snapshot fields.

## Handling Inconsistent State

If the interface and the actual state do not match:

1. Wait a few seconds to see whether it recovers on its own.
2. Refresh the page or restart the app.
3. Use "Reset working state".

## Limits

- Waiting for approval is a **pause**, not an ending — it is still within the session lifecycle.
- State belongs on the list and event pages too, not only the detail page.
- The event stream carries the timeline while the runtime snapshot carries the present moment; they are not the same layer.
