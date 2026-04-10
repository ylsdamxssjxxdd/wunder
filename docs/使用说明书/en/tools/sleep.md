---
title: Sleep and Yield
summary: The semantic difference between `sleep` and `sessions_yield`.
read_when:
  - You need to wait for a while, or temporarily yield control of the current turn
source_docs:
  - src/services/tools/sleep_tool.rs
  - src/services/tools/sessions_yield_tool.rs
updated_at: 2026-04-10
---

# Sleep and Yield

This page covers two different tools:

- `sleep`
- `sessions_yield`

Do not mix them up.

## `sleep`

### Minimum arguments

```json
{
  "seconds": 1.5
}
```

### Success result

```json
{
  "ok": true,
  "action": "sleep",
  "state": "completed",
  "summary": "Slept for 1.5 seconds.",
  "data": {
    "requested_seconds": 1.5,
    "elapsed_ms": 1502,
    "reason": null
  }
}
```

It means: **the current turn actually blocked and waited for a period of time.**

## `sessions_yield`

### Minimum arguments

```json
{
  "message": "The task was submitted and is waiting for an external result"
}
```

### Success result

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

It means: **the current turn yielded control and did not produce a final reply.**

## How to choose

- For a simple polling delay, use `sleep`
- If you need to tell the system "stop this turn here for now," use `sessions_yield`
