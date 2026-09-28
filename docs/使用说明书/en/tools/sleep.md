---
title: Sessions Yield
summary: Semantics and use cases of `sessions_yield`.
read_when:
  - You need to temporarily yield control of the current turn and wait for an external result
source_docs:
  - src/services/tools/sessions_yield_tool.rs
updated_at: 2026-09-28
---

# Sessions Yield

`sessions_yield` means: **the current turn yields control and does not produce a final reply.**

The standalone `sleep` tool has been removed: command execution blocks by default and returns the result; to wait for a background command, use the `command_session` poll window (`yield_time_ms`) instead of idle waiting.

## Minimum arguments

```json
{
  "message": "The task was submitted and is waiting for an external result"
}
```

## Success result

```json
{
  "ok": true,
  "action": "sessions_yield",
  "state": "yielded",
  "summary": "Yielded the current turn and is waiting.",
  "data": {
    "status": "yielded",
    "message": "The task was submitted and is waiting for an external result"
  },
  "meta": {
    "turn_control": {
      "kind": "yield",
      "message": "The task was submitted and is waiting for an external result"
    }
  }
}
```

## When to use

- Use it to explicitly tell the system "stop this turn here and wait for an external resume."
- Not needed for waiting on commands: `execute_command` blocks by default; poll background commands with `command_session`.
