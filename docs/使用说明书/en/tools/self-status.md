---
title: Self Status
summary: Checks the current session's model, context usage, round progress, and event counts, for deciding whether to compress context or wrap up.
updated_at: 2026-10-13
---

The tool is named `自我状态`, also known as `self_status`. It is a read-only diagnostic tool that performs no business action. It reports how far the run has progressed, how much context remains, which model is in use, and what this turn has done.

It is not in the tool set by default; an administrator must tick it on the capability panel for it to appear.

## When to use it

- To confirm which model the current session uses, how much context is occupied, and how far the rounds have progressed.
- To judge whether context should be compressed or the turn should be wrapped up.
- To get event counts (model requests, tool calls, approvals) in order to review what this turn did.

It is read-only and changes no state, so do not use it for business actions.

## How to use it

The level of detail is selectable as needed:

- `standard` by default, enough for everyday checks; `basic` is terser, and `full` is the most complete.
- Besides those three, `lite`, `summary`, `detail`, `detailed`, and `verbose` are also accepted and are normalized to roughly the matching level; an unrecognized value falls back to `standard`.
- To see a recent event list, turn on `include_events` (the `full` level turns it on by default); `events_limit` controls how many come back, in the range 1–200, 20 by default.

## What the result contains

It returns a status object directly, rather than the shared success shell:

- Session identity: user, session, workspace, agent, and whether the user is an administrator.
- The model in use: inferred from the most recent model-request event, and empty when there is no event.
- The configured default model: empty when no default is configured.
- Several groups of context-usage and peak values.
- Monitor status, stage, and summary.
- Counts related to user rounds and model rounds.
- Total event count, the most recent event id, and counts by type; with the event list on, it also carries the most recent few entries.
- The orchestrator's session runtime snapshot, empty when there is no orchestrator.

A few points worth noting:

- The data comes from the in-memory runtime state, not a database ledger. For the full history, go to the thread trace; do not expect the event count here to cover the whole lifecycle.
- The event cap only trims the recent-event list; the total event count is still the full count in the buffer.
- The "model in use" comes from the event stream and may not match the configured default.
- The context values use several different measures, returned side by side without converting between them; do not mix numbers from different measures.

## Diagnosing problems

- A bad argument is returned with its error text unchanged, and the execution layer records it as a failure.
- An unavailable monitor or a missing orchestrator is not an error; the corresponding fields are simply empty.
- It works across Pupa, Beehive, Helm, and Hull; it is read-only, triggers no approval, and does not run in the sandbox.

## Related pages

- Thread-level session management: [Thread Control](/docs/en/tools/thread-control/)
- Tool overview and enablement tiers: [Tools](/docs/en/tools/index/)
