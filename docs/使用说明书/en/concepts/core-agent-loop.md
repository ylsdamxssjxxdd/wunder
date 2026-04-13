---
title: Core Agent Loop
summary: The Core Agent Loop explains why Wunder treats threads as first-class runtime units, and why the main execution path must be stable, recoverable, and convergent.
read_when:
  - You want to understand why threads are Wunder's first-class runtime units
  - You need to determine where execution logic should land when integrating, troubleshooting, or designing tools
source_docs:
  - docs/总体设计.md
---

# Core Agent Loop

The Core Agent Loop is Wunder's main execution path. What it truly solves is not "can the model respond," but "can a thread steadily advance a task to its terminal state."

![Core Agent Loop diagram: input enters thread, model judges, tool acts, state converges, event projection forms the system's main path](/docs/assets/manual/core-agent-loop.svg)

## Bottom Line

- Wunder's basic execution unit is not a single model call, but a continuously running thread.
- Whether a round of execution is healthy depends on whether it can complete "judge, act, observe, continue, converge" within the thread.
- What the frontend sees is an execution projection; the true execution reality always lives in the thread's main path.

## Why It Must Be a Core Concept

Without the Core Agent Loop as a foundation, Wunder would immediately regress into a system that "temporarily assembles context, temporarily calls the model, and temporarily delivers results" for each request. This would bring three consequences:

- Unable to stably express which step an execution has reached, because execution state no longer has a fixed landing point.
- Tool calls, approval waits, and resumption would all become bolted-on logic, difficult to govern.
- Frontend, channels, and monitoring would each maintain their own state judgments, causing the system's semantics to become increasingly chaotic.

## What This Core Truly Protects

- It protects thread semantics. A thread is not a UI label, but the shared boundary for prompt, memory, context, and state.
- It protects execution order. Model calls, tool calls, waiting states, and terminal states must all enter the same chain of facts.
- It protects recovery capability. After disconnection, failure, or waiting for approval, whether the system can continue running depends on whether the main path is stable.

## Key Constraints

| Constraint | Purpose |
|------|------|
| Thread semantics must be stable | The main path cannot be broken by presentation layer, debugging logic, or temporary compatibility branches |
| Terminal states must converge | An execution round must explicitly land on a terminal state like `completed/failed/cancelled/rejected` |
| Recovery must continue within the thread | Disconnection resumption, approval waiting, background wakeup cannot escape thread semantics |
| Projections cannot rewrite reality | Frontend state is just a snapshot and projection, not the backend thread's reality |

## Design Priorities

### Priority 1: Thread is more important than "current response"

Users see this round's reply, but what the system must truly maintain is whether this thread can continue to work reliably in the future.

### Priority 2: Actions must return to the same chain

Model calls, tool observations, approval waits, session handoffs, and background returns—these actions appear scattered, but must all land back on the thread's main path. Otherwise the state machine becomes fragmented.

### Priority 3: Terminal state is not a UI feeling, it's a system fact

Whether a round has ended cannot be judged by "the page seems to have stopped," but must rely on explicit terminal state events and runtime convergence.

## Common Misconceptions

- A thread is not an alias for a session. Sessions carry interaction history; threads carry execution state and runtime semantics.
- Streaming output is not the loop itself; streaming is just the loop's continuous external projection.
- Swarms, sub-agents, and scheduled tasks can all trigger execution, but none can replace thread semantics.

## Boundaries with Other Cores

- Difference from [Realtime](/docs/en/concepts/core-realtime/): Realtime discusses "how to send thread changes out," the Core Agent Loop discusses "how threads steadily advance internally."
- Difference from [Stability](/docs/en/concepts/core-stability/): Stability discusses "how to handle failures," the Core Agent Loop discusses "how to organize the main path."
- Difference from [Memory](/docs/en/concepts/core-memory/): Memory discusses "what to bring when thread initializes," the Core Agent Loop discusses "how the thread continues to run afterward."

## Further Reading

- [Core Overview](/docs/en/concepts/)
- [Sessions and Rounds](/docs/en/concepts/sessions-and-rounds/)
- [Streaming Execution](/docs/en/concepts/streaming/)
- [Presence and Runtime](/docs/en/concepts/presence-and-runtime/)