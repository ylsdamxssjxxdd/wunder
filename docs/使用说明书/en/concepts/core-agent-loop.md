---
title: Agent Loop
summary: You state the goal once and the engine runs the remaining steps; the loop advances round by round until it reaches a deliverable result or hits a brake.
updated_at: 2026-10-13
---

You state the goal once and the in-between steps need no human steering. It is not a question-and-answer exchange; the engine calls the model again and again, and each call can run a batch of tools, until it reaches a deliverable result or hits an explicit brake.

## How the loop runs

A task starts with admission, and the engine enters the main loop. Each model round does three things: first it checks whether it should yield and whether the round cap has been exceeded, compressing the context when necessary; then it calls the model and gets back a piece of text or a batch of tool calls; if there are tool calls, it runs them in parallel or exclusively, writes the results back into the history as observations, and moves to the next round; if there are no tool calls, or a brake is hit, the round lands and the loop exits.

The granularity follows [Sessions and Rounds](/docs/en/concepts/sessions-and-rounds/):

- One model round is one model call by the engine.
- A single round can contain several tool calls, and tool calls are not counted as separate rounds.
- Each round corresponds to one visible output in the interface; a tool batch is a separate entry in the timeline but is not a separate round.

## When it stops on its own

The loop does not run forever; several guardrails bring it to a halt:

- **Model round cap**: non-administrator accounts default to at most 1000 rounds, and it stops once they are used up.
- **Per-round tool-call budget**: at most 10000 tool calls in a round; memory recall 2000 and library-query calls 2000 (relaxable to 10000 when necessary), and calls of a type beyond its limit are rejected.
- **Repeated-failure circuit breaker**: it stops when the same failure fingerprint recurs — 5 times for non-retryable errors, 2 times for parameter or protocol errors, and 4 times for "succeeded without progress"; after stopping it relays the failure reason to you.
- **Context-overflow self-healing**: at most 8 recovery attempts, each doing a corrective compression first and then retrying.
- **Empty-output protection**: one automatic retry (resampling with reasoning effort turned off); if it is still empty, it errors out and stops.
- **Timeouts**: model and tools default to 120 s each, with an additional idle watchdog; a timeout counts as a failure toward the circuit breaker.
- **Session lock**: the same thread runs serially, with a lock TTL of 120 s and a 5 s heartbeat; if the holder exits abnormally, the TTL releases it.
- **Active-session cap**: non-administrators are limited to 300, and beyond that requests queue or are told to try again later.
- **Tool parallelism**: at most 4; read-only tools run in parallel and exclusive tools run serially.

Approval and risk tiering are independent of the loop: write or execute calls suspend and wait for your decision, and approval results are cached by tool category for 600 s so the same kind of operation is not asked about again and again.

## Where each round lands

Every round of the loop must finally land on a result, which the interface state reflects:

- **Completed normally**: a deliverable reply is produced.
- **Rejected**: the request was not admitted (busy, quota exhausted, invalid request).
- **Cancelled**: you stopped it yourself.
- **Failed**: the execution errored (tool guardrail, no-progress guard, model error).

While it waits for your input or for an approval, the thread is not in a terminal state; it is still within its lifetime.

## The thread is the unit of execution

- An agent can hold several independent task threads at once; scheduling, locking, and real-time state are all isolated per thread, and different threads do not block one another.
- Once a thread is set, the system prompt is frozen and long-term memory is injected only once at initialization and never rewritten afterwards. This keeps long tasks behaviorally stable and lets the model-side cache be reused.
- A subagent is a unit of work the main agent creates on the fly; it runs its own loop, passes results back to the parent thread, and is reclaimed when the task ends. This is the only form of collaboration.

## When to wait, when to step in, when to open a new thread

Wait when you see "running tool" or several tool entries appearing one after another, which means the loop is advancing normally. Sending the same sentence again does not speed it up; it only queues another user round.

Step in when:

- An approval pops up, so decide whether to approve or reject.
- The inquiry panel asks, and the thread moves to waiting for human input.
- The direction is wrong, so add an instruction, and the new user round carries the new context.
- There has been no output for a long time while the state is still running, so stop it and restate the goal.

Open a new thread when the topic changes, the context is already very long, or you need a clean prompt base (after you change an agent's setup or memory, only a new thread picks it up).

## A few clarifications

- The claim that every tool call counts as a round is wrong — a tool batch belongs to the same model round.
- The loop does not retry without limit; repeated failures, progress-free successes, overruns, and empty output all have circuit breakers.
- A still interface does not mean the loop has stopped; check the state first: waiting for approval, waiting for input, and queued are all still within the lifetime.
- Streaming output is only a way of presenting things; the truth of the loop is in the thread ledger — see [Streaming Execution](/docs/en/concepts/streaming/).
- A subagent does not share the parent thread's context; it is an independent thread with an independent loop, interacting only through callbacks and a mailbox.

## Related pages

- [Sessions and Rounds](/docs/en/concepts/sessions-and-rounds/) — numbering and timeline entries
- [Context Compression](/docs/en/concepts/core-context-compression/) — the second step in the loop
- [Stability](/docs/en/concepts/core-stability/) — failure and recovery design
- [Boundary Handling](/docs/en/concepts/boundary-handling/) — truncation, timeouts, and rejection
- [Session State Reference](/docs/en/reference/stream-events/) — states and event fields
