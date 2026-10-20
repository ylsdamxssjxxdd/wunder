---
title: Self Status
summary: The basic summary, administrator diagnostic mode, compact model result, and data sources of `self_status`.
updated_at: 2026-10-16
---

The canonical name is `自我状态` and the alias is `self_status`. It is read-only: it performs no business action, triggers no approval, and does not run in the sandbox. By default it reports the current session's stage, status, model round, and context use, for deciding whether to compress context or wrap up. It is not in the tool set by default; an administrator must tick it on the capability panel for it to appear.

## Arguments

The model call accepts one optional `detail_level` argument:

- `basic` is the default and supports next-step decisions.
- `full` is only available to an administrator for an explicit diagnostic request; it includes a bounded set of recent events and system resource metrics.
- Other levels and event or system-metric switches are rejected.

## Return fields

The execution layer retains the authorized diagnostic object; the model context always receives only a compact next-step summary:

- `stage`, `status`, `round`, and `context_tokens`.
- `context_ratio` or `last_error` when runtime data supplies them.
- It does not echo session identity, model endpoints, a full runtime snapshot, raw events, or system metrics; those remain in execution audit and administrator full diagnostics.

A few points worth noting:

- The data comes from the in-memory runtime state, not a database ledger. For the full history, go to the thread trace; do not expect the event count here to cover the whole lifecycle.
- Full diagnostics cap recent events at 20 so diagnostic data stays bounded.
- The "model in use" comes from the event stream and may not match the configured default.
- The context values use several different measures, returned side by side without converting between them; do not mix numbers from different measures.

## Errors

- A bad argument is returned with its error text unchanged, and the execution layer records it as a failure.
- An unavailable monitor or a missing orchestrator is not an error; the corresponding fields are simply empty.
- It works across Pupa, Beehive, Helm, and Hull.
