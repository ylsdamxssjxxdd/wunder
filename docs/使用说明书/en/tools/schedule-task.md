---
title: Schedule Task
summary: Recommended `schedule_task` usage, thread delivery semantics, and recurring behavior.
read_when:
  - You need to create, update, inspect, or trigger scheduled jobs
source_docs:
  - src/services/tools/dispatch.rs
  - src/services/cron.rs
updated_at: 2026-04-29
---

# Schedule Task

`schedule_task` creates, updates, inspects, runs, enables, and disables scheduled jobs.

It is a deliberate exception in the current tool system:

- the input side supports model-friendly flat fields
- the success side returns compact scheduling objects
- it does not use the unified `ok/action/state/summary/data` envelope

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

## Recommended style

Prefer flat fields first instead of building a full nested `job` object:

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
  "message": "Run the inspection",
  "session": "isolated"
}
```

## Field guide

- `action`: operation to perform.
- `job_id`: stable job identifier reused for later management calls.
- `name`: short display name.
- `schedule_text`: preferred shortcut using natural language or cron text.
- `schedule`: precise `at/every/cron` object when needed.
- `message`: content the agent should receive when the job fires.
- `session`: execution thread strategy, either `main` or `isolated`.
- `enabled`: whether the job starts active.

## `session` semantics

- `main`
  At fire time, send the message into the agent's **current main thread**.
  It does not keep using the old thread captured when the job was created.

- `isolated`
  At fire time, run in a fresh isolated thread first, then send the result back into the agent's **current main thread**.

If the agent does not currently have a bound main thread, the runtime falls back to the job's stored `session_id`.

## Do recurring jobs pile up?

No. They do not accumulate one backlog item per missed tick.

Current behavior:

- one recurring job can have at most one active run at a time
- if a job is scheduled `every 1s` but one run takes much longer than 1 second, it will not fan out into many concurrent copies
- missed intervals are skipped or coalesced, and the next run is advanced from the current time base

This means recurring jobs behave more like "keep the latest cadence" than "replay every missed interval".

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
    "next_run_at_text": "2026-04-29T10:00:00+08:00"
  },
  "user_jobs": {
    "total": 3,
    "enabled": 2,
    "running": 1,
    "next_run_at": 1760000000,
    "next_run_at_text": "2026-04-29T10:00:00+08:00"
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
    "next_run_at": "2026-04-29T10:00:00+08:00",
    "last_run_at": null,
    "last_status": null
  }
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
      "next_run_at": "2026-04-29T10:00:00+08:00",
      "last_run_at": null,
      "last_status": null
    }
  ]
}
```

## Notes

- If both `schedule_text` and `schedule` are provided, `schedule` wins.
- `schedule.every_ms` must be at least `1000`.
- Arguments must be a complete JSON object. If the JSON is incomplete, the tool now reports invalid arguments directly.
