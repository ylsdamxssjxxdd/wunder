---
title: Self Status
summary: `self_status` is used to inspect the current session, model, events, and thread runtime state.
read_when:
  - You need to understand exactly where the current session is in its execution flow
source_docs:
  - src/services/tools/self_status_tool.rs
updated_at: 2026-04-10
---

# Self Status

`self_status` is a diagnostic tool.  
Unlike most tools, it returns a raw status object on success rather than the unified `ok/action/state/summary/data` envelope.

## Minimum arguments

```json
{
  "detail_level": "standard"
}
```

## Common arguments

- `detail_level`: `basic`, `standard`, or `full`
- `include_events`
- `events_limit`
- `include_system_metrics`

## Return structure

```json
{
  "tool": "self_status",
  "detail_level": "standard",
  "session": {
    "user_id": "user_xxx",
    "session_id": "sess_xxx",
    "workspace_id": "user_xxx",
    "agent_id": "agent_docs",
    "is_admin": false
  },
  "model": {
    "active": { ... },
    "configured_default": { ... }
  },
  "context": {
    "context_occupancy_tokens": 12345,
    "context_overflow": false,
    "monitor_context_tokens": 12000,
    "monitor_context_tokens_peak": 18000
  },
  "monitor": {
    "status": "running",
    "stage": "tool_call",
    "summary": "Waiting for a subagent result",
    "updated_time": 1760000000
  },
  "thread": { ... },
  "rounds": {
    "user_rounds": 3,
    "model_rounds_peak": 12,
    "event_user_round_peak": 3
  },
  "events": {
    "total": 48,
    "last_event_id": 128,
    "counts": {
      "llm_request": 5,
      "tool_call": 12,
      "tool_result": 12,
      "approval_request": 0,
      "approval_resolved": 0
    },
    "by_type": { ... },
    "recent_limit": 20,
    "recent": [ ... ]
  },
  "system_metrics": { ... }
}
```

## Key points

- `detail_level=full` usually includes more events and system metrics
- `events.recent` is the fastest way to see what happened most recently
- `thread` and `monitor` are the most useful sections when you need to know whether the current thread is still running and which stage it is stuck in
