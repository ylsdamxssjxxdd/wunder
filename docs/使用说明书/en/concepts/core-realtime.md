---
title: Realtime
summary: The realtime core explains why Wunder must continuously project thread changes to the outside world, and strictly distinguishes between event projection and backend truth.
read_when:
  - You are working on WebSocket, streaming output, or state synchronization
  - You want to understand the boundaries of event streams, snapshot compensation, and disconnection recovery
source_docs:
  - docs/总体设计.md
---

# Realtime

In Wunder, realtime is not simply "text appearing gradually", but rather enabling the frontend and external systems to continuously receive trustworthy, recoverable, and convergent runtime state changes.

![Realtime diagram: thread facts projected to client state through event streams and recovery mechanisms](/docs/assets/manual/core-realtime.svg)

## Key Takeaways

- The focus of realtime is not character streams, but thread state streams.
- Event streams must be able to express processes, final states, and recovery, not just keep spitting incremental text.
- What the client sees is a projection; projections must stay close to truth, but cannot replace truth.

## Why It Must Be a Core Principle

Without realtime as a core principle, the system would exhibit three obvious problems:

- Users can only receive final results very late, with the process being invisible.
- After disconnection, users don't know if execution continued or what was missed.
- The frontend can only guess "still running or already finished", making state machines very fragile.

## What This Core Truly Protects

- Protects user perception: Let users know what the system is doing, rather than just waiting for the final sentence.
- Protects client state machines: Enable lists, details, workflows, and approval areas to work based on stable events.
- Protects recovery capability: Disconnection, reconnection, replay, and resume all have a unified basis.

## Key Constraints

| Constraint | Purpose |
|------|------|
| Frontend consumes event streams but does not define truth | Prevents UI state from polluting backend semantics in reverse |
| Final states must have explicit events | Cannot let clients guess completion by timeout |
| Recovery mechanisms must formally exist | After disconnection, must be able to resume, replay, and rebuild snapshots |
| Event terminology must be stable | Cannot invent a set of state names for a specific page |

## Design Priorities

### Priority 1: Streaming is only part of realtime

Incremental text, tool output, approval waiting, thread state, final state events, and recovery compensation—combined, these constitute realtime.

### Priority 2: Realtime systems must support "continuing from where you left off", not just "starting from the beginning"

Systems that only support persistent connection push but not recovery are inherently still fragile.

### Priority 3: Event semantics must be designed for multi-client reuse

The same event must be consumable by the chat panel, admin pages, desktop shell, and external systems, so it cannot serve only one page.

## Common Misconceptions

- `final` or the final reply is not the only truth; final state determination must combine complete event semantics.
- "Can stream characters" does not equal "has realtime capability"; state synchronization and recovery are equally critical.
- Busy/idle states on the frontend are projections, not substitutes for the backend state machine.

## Boundaries with Other Cores

- Difference from [Agent Loop](/docs/en/concepts/core-agent-loop/): The loop defines how threads advance internally, realtime defines how these advances become visible externally.
- Difference from [Observability](/docs/en/concepts/core-observability/): Realtime focuses on online consumption, observability focuses on post-hoc explanation and review.
- Difference from [Stability](/docs/en/concepts/core-stability/): Realtime cares whether state can be delivered in time, stability cares whether the system can continue working during exceptions.

## Further Reading

- [Core Overview](/docs/en/concepts/)
- [Streaming Execution](/docs/en/concepts/streaming/)
- [Chat WebSocket](/docs/en/integration/chat-ws/)
- [Stream Events Reference](/docs/en/reference/stream-events/)