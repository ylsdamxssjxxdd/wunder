---
id: task_csv_metric_summary
name: CSV metric summary
suite: data-analysis
category: csv_analysis
grading_type: automated
timeout_seconds: 240
runs_recommended: 2
difficulty: medium
required_tools:
  - read_file
  - write_file
tags:
  - csv
  - analysis
  - report
languages:
  - en
workspace_files:
  - path: input/metrics.csv
    content: |
      period,segment,requests,errors,latency_ms
      2026-01,A,1200,24,180
      2026-01,B,900,9,160
      2026-02,A,1500,45,210
      2026-02,B,950,19,175
      2026-03,A,1800,36,205
      2026-03,B,1000,40,230
  - path: input/requirements.md
    content: |
      Summarize request volume, error rate, and latency by segment.
      Flag any segment-month with error_rate >= 0.03.
      Recommend two concrete follow-up checks.
---

## Prompt

Read `{attempt_root}/input/metrics.csv` and `{attempt_root}/input/requirements.md`.

Create:

1. `{attempt_root}/output/summary.json`
2. `{attempt_root}/output/insights.md`

`summary.json` must be valid JSON with this shape:

```json
{
  "total_requests": 0,
  "total_errors": 0,
  "overall_error_rate": 0.0,
  "segments": {
    "A": {"requests": 0, "errors": 0, "avg_latency_ms": 0.0},
    "B": {"requests": 0, "errors": 0, "avg_latency_ms": 0.0}
  },
  "flagged_periods": []
}
```

Rules:

- `overall_error_rate` is `total_errors / total_requests`.
- `avg_latency_ms` is the arithmetic mean for each segment.
- `flagged_periods` contains objects with `period`, `segment`, and `error_rate` for rows with error rate at least `0.03`.
- `insights.md` must include the highest-risk segment and two follow-up checks.

Only write results under `{attempt_root}/output`.

## Expected Behavior

The agent should compute aggregate metrics from CSV input, identify rows that cross the risk threshold, and produce both machine-readable JSON and a concise human-readable summary.

## Grading Criteria

- [ ] Correct total request and error counts
- [ ] Correct per-segment aggregates and average latency
- [ ] Correct flagged periods by threshold
- [ ] Human summary includes risk and follow-up checks

## Automated Checks

```python
def grade(transcript, workspace_path):
    import json
    import os

    scores = {
        "summary_exists": 0.0,
        "insights_exists": 0.0,
        "totals_correct": 0.0,
        "segments_correct": 0.0,
        "flags_correct": 0.0,
    }

    summary_path = os.path.join(workspace_path, "output", "summary.json")
    insights_path = os.path.join(workspace_path, "output", "insights.md")
    if os.path.exists(summary_path):
        scores["summary_exists"] = 1.0
    if os.path.exists(insights_path):
        scores["insights_exists"] = 1.0
    if not os.path.exists(summary_path):
        return scores

    with open(summary_path, "r", encoding="utf-8") as fp:
        data = json.load(fp)

    if data.get("total_requests") == 7350 and data.get("total_errors") == 173:
        rate = float(data.get("overall_error_rate", -1))
        if abs(rate - (173 / 7350)) < 0.0001:
            scores["totals_correct"] = 1.0

    segments = data.get("segments") or {}
    seg_a = segments.get("A") or {}
    seg_b = segments.get("B") or {}
    if (
        seg_a.get("requests") == 4500
        and seg_a.get("errors") == 105
        and abs(float(seg_a.get("avg_latency_ms", -1)) - 198.3333333333) < 0.01
        and seg_b.get("requests") == 2850
        and seg_b.get("errors") == 68
        and abs(float(seg_b.get("avg_latency_ms", -1)) - 188.3333333333) < 0.01
    ):
        scores["segments_correct"] = 1.0

    flags = {
        (str(item.get("period")), str(item.get("segment")))
        for item in (data.get("flagged_periods") or [])
        if isinstance(item, dict)
    }
    if flags == {("2026-02", "A"), ("2026-03", "B")}:
        scores["flags_correct"] = 1.0

    return scores
```
