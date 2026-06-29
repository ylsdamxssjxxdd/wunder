---
id: task_calendar_conflict_plan
name: Calendar conflict plan
suite: office-workflow
category: scheduling
grading_type: automated
timeout_seconds: 240
runs_recommended: 2
difficulty: medium
required_tools:
  - read_file
  - write_file
tags:
  - calendar
  - scheduling
  - planning
languages:
  - en
workspace_files:
  - path: input/events.csv
    content: |
      event_id,title,start,end,priority
      EV-1,Planning,09:00,10:00,high
      EV-2,Review,09:30,10:30,medium
      EV-3,Focus,10:30,11:30,high
      EV-4,Sync,11:00,11:30,low
  - path: input/rules.md
    content: |
      Keep high-priority events at their original times.
      Medium-priority events may move in 30-minute increments.
      Low-priority events may be cancelled if they overlap a high-priority event.
      Prefer the earliest later available valid time in 30-minute increments.
---

## Prompt

Resolve the schedule in `{attempt_root}/input/events.csv` using `{attempt_root}/input/rules.md`.

Create:

1. `{attempt_root}/output/schedule_plan.json`
2. `{attempt_root}/output/notice.md`

`schedule_plan.json` must be a JSON array. Each item must include:

```json
{
  "event_id": "",
  "decision": "keep|move|cancel",
  "start": "",
  "end": "",
  "reason": ""
}
```

Rules:

- Keep `EV-1` and `EV-3`.
- Move `EV-2` to the earliest later valid 30-minute-increment slot that avoids conflict.
- Cancel `EV-4` because it overlaps high-priority focus time.
- `notice.md` must briefly summarize the changes.

Only write results under `{attempt_root}/output`.

## Expected Behavior

The agent should reason over time intervals, apply priority rules, and produce a clear structured schedule change plan.

## Grading Criteria

- [ ] Keeps high-priority events
- [ ] Moves medium-priority event to a valid slot
- [ ] Cancels low-priority conflict
- [ ] Produces a readable notice

## Automated Checks

```python
def grade(transcript, workspace_path):
    import json
    import os

    scores = {
        "plan_exists": 0.0,
        "notice_exists": 0.0,
        "high_priority_kept": 0.0,
        "medium_moved": 0.0,
        "low_cancelled": 0.0,
    }

    plan_path = os.path.join(workspace_path, "output", "schedule_plan.json")
    notice_path = os.path.join(workspace_path, "output", "notice.md")
    if os.path.exists(plan_path):
        scores["plan_exists"] = 1.0
    if os.path.exists(notice_path):
        scores["notice_exists"] = 1.0
    if not os.path.exists(plan_path):
        return scores

    with open(plan_path, "r", encoding="utf-8") as fp:
        items = json.load(fp)

    mapping = {item.get("event_id"): item for item in items if isinstance(item, dict)}
    if (
        mapping.get("EV-1", {}).get("decision") == "keep"
        and mapping.get("EV-1", {}).get("start") == "09:00"
        and mapping.get("EV-3", {}).get("decision") == "keep"
        and mapping.get("EV-3", {}).get("start") == "10:30"
    ):
        scores["high_priority_kept"] = 1.0

    ev2 = mapping.get("EV-2", {})
    if ev2.get("decision") == "move" and ev2.get("start") == "11:30" and ev2.get("end") == "12:30":
        scores["medium_moved"] = 1.0

    if mapping.get("EV-4", {}).get("decision") == "cancel":
        scores["low_cancelled"] = 1.0

    return scores
```
