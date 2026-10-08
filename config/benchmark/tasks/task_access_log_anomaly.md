---
id: task_access_log_anomaly
name: Access log anomaly triage
suite: ops-observability
category: log_analysis
grading_type: automated
timeout_seconds: 240
runs_recommended: 2
difficulty: medium
required_tools:
  - read_file
  - write_file
tags:
  - logs
  - anomaly
  - observability
languages:
  - en
workspace_files:
  - path: input/access.log
    content: |
      2026-01-01T00:00:00Z GET /health 200 12
      2026-01-01T00:00:01Z GET /api/items 200 145
      2026-01-01T00:00:02Z POST /api/items 201 220
      2026-01-01T00:00:03Z GET /api/items 500 980
      2026-01-01T00:00:04Z GET /api/items 502 1100
      2026-01-01T00:00:05Z GET /api/items 504 1250
      2026-01-01T00:00:06Z GET /api/search 200 410
      2026-01-01T00:00:07Z GET /api/search 200 430
      2026-01-01T00:00:08Z GET /api/search 200 1200
      2026-01-01T00:00:09Z GET /health 200 10
  - path: input/policy.md
    content: |
      Treat status codes >= 500 as server errors.
      Treat latency >= 1000 ms as a latency spike.
      A path is high risk when it has at least two server errors or at least one latency spike.
---

## Prompt

Analyze `{attempt_root}/input/access.log` using `{attempt_root}/input/policy.md`.

Create:

1. `{attempt_root}/output/anomalies.json`
2. `{attempt_root}/output/runbook.md`

`anomalies.json` must be valid JSON with this shape:

```json
{
  "total_lines": 0,
  "server_error_count": 0,
  "latency_spike_count": 0,
  "high_risk_paths": [],
  "events": []
}
```

Rules:

- `high_risk_paths` is a sorted list of path strings.
- `events` must include one object per anomalous log line with `timestamp`, `path`, `status`, `latency_ms`, and `reason`.
- `reason` should be `server_error`, `latency_spike`, or `server_error_and_latency_spike`.
- `runbook.md` must state the high-risk path and at least two immediate checks.

Only write results under `{attempt_root}/output`.

## Expected Behavior

The agent should parse unstructured access logs, apply deterministic anomaly rules, and write a compact triage artifact suitable for operations review.

## Grading Criteria

- [ ] Correctly counts log lines
- [ ] Correctly identifies server errors
- [ ] Correctly identifies latency spikes
- [ ] Correctly names high-risk paths
- [ ] Produces a useful runbook

## Automated Checks

```python
def grade(transcript, workspace_path):
    import json
    import os

    scores = {
        "anomalies_exists": 0.0,
        "runbook_exists": 0.0,
        "counts_correct": 0.0,
        "paths_correct": 0.0,
        "events_reasonable": 0.0,
    }

    anomalies_path = os.path.join(workspace_path, "output", "anomalies.json")
    runbook_path = os.path.join(workspace_path, "output", "runbook.md")
    if os.path.exists(anomalies_path):
        scores["anomalies_exists"] = 1.0
    if os.path.exists(runbook_path):
        scores["runbook_exists"] = 1.0
    if not os.path.exists(anomalies_path):
        return scores

    with open(anomalies_path, "r", encoding="utf-8") as fp:
        data = json.load(fp)

    if (
        data.get("total_lines") == 10
        and data.get("server_error_count") == 3
        and data.get("latency_spike_count") == 3
    ):
        scores["counts_correct"] = 1.0

    if data.get("high_risk_paths") == ["/api/items", "/api/search"]:
        scores["paths_correct"] = 1.0

    events = data.get("events") or []
    item_events = [item for item in events if item.get("path") == "/api/items"]
    search_events = [item for item in events if item.get("path") == "/api/search"]
    reasons = {str(item.get("reason")) for item in events}
    if len(item_events) == 3 and len(search_events) == 1 and "server_error_and_latency_spike" in reasons:
        scores["events_reasonable"] = 1.0

    return scores
```
