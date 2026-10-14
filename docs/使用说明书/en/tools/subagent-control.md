---
title: Subagent Control
summary: Letting the main agent hand a self-contained piece of work to subagents, then track, wait on, guide, and reclaim them.
updated_at: 2026-10-13
---

The tool is `subagent_control`.

A subagent is a unit of work the main agent creates temporarily: it dispatches one task thread and reclaims it once the task ends. This is currently the only form of collaboration.

## When to use it

- A piece of work can be finished on its own and only a summary needs to come back: dispatch one subagent.
- One goal needs several parallel attempts or mutual review: dispatch a batch at once.
- Work already dispatched needs its status checked, its result waited on, correction, interruption, or shut-down.
- A subagent itself needs to report back to its parent thread (only a subagent can do this).

All of these actions hang off the current thread. Creating, switching, and archiving threads themselves is a separate matter, handled by [Thread Control](/docs/en/tools/thread-control/).

## How to use it

Each call specifies one action:

- `list`: list the child threads under a parent thread.
- `history`: read the message history of a child thread.
- `send`: append a round of instructions to a child thread.
- `report`: a child thread reports back to its parent.
- `spawn`: dispatch one subagent.
- `batch_spawn`: dispatch a batch at once, with a strategy and handling for the remaining branches.
- `status`: read a target's current snapshot without waiting.
- `wait`: wait until the completion condition is met or a timeout occurs.
- `interrupt`: cancel a run.
- `close`: close a session, optionally cascading.
- `resume`: restore it to active; with a message it is the same as one append instruction.

When no target is given, the current session is treated as the parent thread by default; you can also point at a target using a dispatched run id, session id, or batch id.

When dispatching, you can also specify:

- `label`: a recognizable name for the child thread.
- `agent_id`, `model`: override the subagent or the model it uses; both are checked against the current user's agent ownership.
- `run_timeout_seconds`: the timeout of a single run; in a synchronous dispatch it is also the wait duration.
- `fork_turns`: copy the most recent rounds of user context, 0–16, 0 by default.
- `context_summary`: attach a background summary.
- `cleanup`: how to wrap up, `keep` (the default, kept and inspectable) or `delete` (cleaned up).

## What it handles

**How context is inherited.** The copied context and background summary are reference data and never change the subagent's system prompt: the parent thread's most recent rounds are prepended to the task text as quoted background, copying at most 256 history entries and 64 KiB in total, with anything beyond marked as truncated. A `fork_turns` above 16, or a summary above 16384 bytes, fails immediately; when `fork_turns` is 0 and no summary is written, no history is read. A subagent inherits its parent session's tool set by default, and its tool system prompt stays frozen once it is first determined; long-term memory and background injection happen only once, at initialization.

**Dispatching one and dispatching a batch.**

- One: when the timeout is 0 or negative it returns at once and runs in the background; when it is positive it waits synchronously for a result, sending progress every 5 seconds and returning a timeout if the wait expires.
- A batch: context is prepared for every task first, and if any single preparation fails the whole batch is not dispatched (no half batch is left behind). It then dispatches one by one, reclaiming any that fails to start and marking it failed. The completion condition follows the strategy: run all in parallel / review then merge → wait for all; first success → finished at the first success. Once it finishes early, the remaining branches are kept by default, or interrupted by default under the first-success strategy.

**Waiting, sending, and reporting.** A wait subscribes to the completion signal first, and when there is no signal it polls at a fixed interval that settles around one second (between 0.2 and 5 seconds). A wait can end for three reasons: the completion condition is met, it times out, or the parent thread receives a message. When a child thread is running its current turn and its mailbox is open, a message sent to it goes straight into that turn; otherwise it starts a new one, and sending a message to itself raises an error. When a child reports back, an idle parent thread is woken automatically to receive it, and submitting the same message again does not enqueue it a second time.

**Interrupting and closing.** An interrupt only requests cancellation of the run and does not change session state; a close sets the session state to closed and cancels the run along with it, and can also cascade to its descendants; a resume sets the state back to active.

## When something goes wrong

- A single call can select at most 200 targets; beyond that it reports that there are too many targets.
- Single-target actions such as reading history or sending a message require exactly one child thread: it first looks up by exact id, then looks for a unique close name under the same parent thread, then falls back to picking the only child thread when there is just one; if it is still not unique it errors and asks you to use the exact id returned by a dispatch or a list.
- A target must share the same root as the caller, and the root thread cannot be a dispatch target; nesting depth is capped at 32 levels.
- A batch holds at most 64 tasks; beyond that the whole batch is rejected rather than truncated.
- Each child thread's outbound messages and receipt cache are bounded, and a full one is treated as a rejection; a list holds at most 200 entries, 50 by default.
- Result previews and status messages in an entry are truncated to 160 characters; the full content is available from the history or that child thread's trail.
- In Pupa this tool is on by default as an extra; the other forms require an administrator to select it.

## Related pages

- Thread management: [Thread Control](/docs/en/tools/thread-control/)
- Tools and result trimming: [Tools](/docs/en/tools/index/)
