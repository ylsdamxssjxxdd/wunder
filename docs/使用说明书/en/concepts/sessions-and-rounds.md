---
title: Sessions and Rounds
summary: wunder splits a conversation into sessions, threads, user rounds, and model rounds.
read_when:
  - You need to understand session state, event streams, and counting conventions
  - You need to distinguish user rounds, model rounds, and token occupancy
source_docs:
  - docs/设计文档/01-系统总体设计.md
  - docs/API文档.md
---

# Sessions and Rounds

wunder does not treat a conversation as a single request. It treats it as a continuously evolving session.

## What is a session?

A session usually contains:

- user input
- model responses
- tool calls
- intermediate events
- the final result

For complex work, one session may last for many turns and include multiple model calls and tool invocations.

## What is a thread?

A thread is the higher-level structure that organizes a session.

The current system requires:

- an agent thread's system prompt must be frozen after it is first determined
- long-term memory may only be injected once during thread initialization
- later rounds continue reusing the same thread context

So a thread is not just a UI label. It is the actual boundary for stability and context consistency.

## User rounds and model rounds

wunder explicitly splits rounds into two layers.

### User rounds

Each message sent by the user counts as one user round.

This represents a clear business-input boundary.

### Model rounds

Each action executed by the model counts as one model round.

Actions include:

- one model call
- one tool call
- one final reply

So one user round usually contains multiple model rounds.

## Why this distinction matters

If you do not separate the two kinds of rounds, these questions blur together:

- how many times did the user actually ask something
- how many steps did the model take to answer that one question
- at which step did a tool explosion happen
- where should current speed, latency, and token statistics be attached

Once user rounds and model rounds are separated, timelines, replay, alerts, and billing-related conventions become much clearer.

## Token statistics

wunder records token occupancy, meaning how many tokens the current context actually occupies.

It is not:

- the total billing cost of a platform
- a simple sum of every provider-side billing field

When you inspect the chat UI, debugging UI, or replay data, you should first read token counts as **current thread context load**.

## Streaming events and termination

During one user round, the event stream usually shows:

- input arrival
- model start
- tool calls and observations
- incremental output
- a termination event

Whether execution truly finished should be judged by termination semantics, not just by whether any streaming text appeared.

If you are integrating or troubleshooting, at minimum keep these separate:

- whether the session is still alive
- whether the current thread runtime is still active
- whether the current round has already emitted a terminating event

## Further reading

- [Streaming Execution](/docs/en/concepts/streaming/)
- [Presence and Runtime](/docs/en/concepts/presence-and-runtime/)
- [Streaming Events Reference](/docs/en/reference/stream-events/)
- [wunder API](/docs/en/integration/wunder-api/)
