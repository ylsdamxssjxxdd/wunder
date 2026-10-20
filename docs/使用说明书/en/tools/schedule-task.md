---
title: Schedule Task
summary: The eight actions and common fields of `schedule_task`, its two execution threads, concurrency and missed-run policy for recurring jobs, and failure diagnosis around queries and the scheduler switch.
updated_at: 2026-10-16
---

Use it when something should not be done only once but repeated on a fixed cadence, or when it should run automatically once at a designated moment: it creates, changes, inspects, runs right away, and enables or disables jobs.

## Actions and fields

A call specifies the action and the job's content. The model schema is action-specific: `add`/`update` expose job fields, `get`/`remove`/`enable`/`disable`/`run` require only `job_id`, and `list`/`status` reject job fields. The flat form uses top-level fields:

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

For precise control over timing, use a nested time rule, which takes a cron expression and a time zone. Use `schedule` or `schedule_text` only for actions that accept scheduling fields; query and control actions reject them:

```json
{
  "action": "add",
  "job_id": "job_cron_demo",
  "schedule": { "kind": "cron", "cron": "*/5 * * * *", "tz": "Asia/Shanghai" },
  "message": "Run the inspection",
  "session": "isolated"
}
```

Supported actions: `add` (create), `update` (change), `remove` (delete), `enable` / `disable` (enable or disable), `get` (view one), `list` (list), `run` (run once right away), and `status` (scheduler status).

Common fields:

- `action`: the action to perform.
- `job_id`: the job's identifier, used for changing, deleting, enabling or disabling, running right away, and querying.
- `name`: the job's display name.
- `schedule_text`: the time rule, written in natural language or cron text.
- `schedule`: used only when you need to express precisely "at a moment / every so often / cron".
- `message`: the message sent to the agent when the job fires.
- `session`: which thread the job runs in, `main` or `isolated` only.
- `enabled`: whether the job is enabled right after creation.

## Execution threads

`session` decides where this happens:

- `main`: runs in the thread the job is bound to. By default it is bound to the thread you created the job in; switching pages later, opening a new thread, or another thread becoming active does not change the binding. When busy it queues into the shared task queue, and you can also stop it from the bound thread.
- `isolated`: runs in a fresh child thread under the bound thread. The result is delivered back to the bound thread as a standalone "completed turn" rather than being fed to the model again as a new user instruction; the child thread and the result turn each have their own identity, and an error does not get written into the previous turn's assistant bubble.

If the bound thread is archived or deleted, or its owning agent does not match, the job fails explicitly rather than quietly picking another thread to stand in.

## Concurrency of recurring jobs

A single recurring job has at most one run active at any moment. Even if it is written as once per second while a single run takes far longer than a second, it will not fan out into a pile of identical concurrent jobs. Missed intervals are skipped or coalesced, and the next run time is pushed forward again from the current time: the behavior keeps the latest cadence rather than catching up on every missed run.

## Errors

- `run` returning `queued: true` only means the asynchronous run has been accepted, not that it has finished. Do not repeatedly poll for it within the turn that created the job; finish the current reply first.
- Querying a job shows the thread it is bound to, whether it is running, and its last result and last error; it displays local-time-zone time, the run rule, and the bound thread, while the raw result stays in the thread log.
- When a time rule provides both `schedule_text` and `schedule`, `schedule` wins; the smallest interval is 1 second. Arguments must be complete JSON; if it is not closed, the arguments are reported invalid directly.
- The server must have scheduling enabled first. When it is off, create, update, enable, and manual run return explicit errors, but the job list can still be read and the scheduler status still shown; a service already running needs a config reload or restart to pick up the change.
- Scheduled runs inherit the approval mode, model, tool capabilities, and workspace configuration of the owning user and agent. An isolated thread keeps the bound thread's explicit tool restrictions and freezes its own starting tool baseline, and still obeys the user's tool permissions: an agent that requires approval does not become fully automatic just because a schedule triggered it.
