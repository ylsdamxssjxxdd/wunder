---
title: Schedule Task
summary: The flat input style, scheduling actions, and compact return shape of `schedule_task`.
read_when:
  - You need to create, update, inspect, or trigger scheduled jobs
source_docs:
  - src/services/tools/dispatch.rs
  - src/services/cron.rs
updated_at: 2026-04-10
---

# Schedule Task

`schedule_task` is a clear exception in the current tool system:

- the input side supports model-friendly flat fields
- the success side returns compact scheduling objects
- **it does not use the unified success envelope**

## Supported actions

- `add`
- `update`
- `remove`
- `enable`
- `disable`
- `get`
- `list`
- `run`
- `status`

## Recommended input style

Prefer flat fields first:

```json
{
  "action": "add",
  "job_id": "job_daily_report",
  "name": "Daily report reminder",
  "schedule_text": "every 5 minutes",
  "message": "Please generate the daily report",
  "session": "main",
  "enabled": true
}
```

Only use the nested `schedule` object when you need exact control:

```json
{
  "action": "add",
  "job_id": "job_cron_demo",
  "schedule": {
    "kind": "cron",
    "cron": "*/5 * * * *",
    "tz": "Asia/Shanghai"
  },
  "message": "Run the inspection"
}
```

## Success results

### `status`

```json
{
  "action": "status",
  "scheduler": {
    "enabled": true,
    "poll_interval_ms": 1000,
    "running_jobs": 1,
    "next_run_at": 1760000000,
    "next_run_at_text": "2026-04-10T10:00:00+08:00"
  },
  "user_jobs": {
    "total": 3,
    "enabled": 2,
    "running": 1,
    "next_run_at": 1760000000,
    "next_run_at_text": "2026-04-10T10:00:00+08:00"
  }
}
```

### `add`, `update`, and `get`

```json
{
  "action": "add",
  "job": {
    "job_id": "job_daily_report",
    "name": "Daily report reminder",
    "enabled": true,
    "schedule": {
      "kind": "every",
      "every_ms": 300000
    },
    "next_run_at": 1760000000,
    "next_run_at_text": "2026-04-10T10:00:00+08:00",
    "last_run_at": null,
    "last_status": null
  },
  "deduped": false
}
```

### `list`

```json
{
  "action": "list",
  "jobs": [
    {
      "job_id": "job_daily_report",
      "name": "Daily report reminder",
      "enabled": true,
      "schedule": { "kind": "every", "every_ms": 300000 },
      "next_run_at": "2026-04-10T10:00:00+08:00",
      "last_run_at": null
    }
  ]
}
```

## Key points

- This is one of the few tools that still does not use `ok/action/state/summary/data`
- Prefer flat input fields when the model writes arguments
- The most important result fields are `job`, `jobs`, `scheduler`, and `user_jobs`
