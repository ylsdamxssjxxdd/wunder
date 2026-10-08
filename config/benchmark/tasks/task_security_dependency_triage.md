---
id: task_security_dependency_triage
name: Security dependency triage
suite: security-triage
category: risk_analysis
grading_type: automated
timeout_seconds: 240
runs_recommended: 2
difficulty: medium
required_tools:
  - read_file
  - write_file
tags:
  - security
  - dependencies
  - triage
languages:
  - en
workspace_files:
  - path: input/findings.json
    content: |
      [
        {"component": "pkg-alpha", "severity": "critical", "reachable": true, "fix_available": true},
        {"component": "pkg-beta", "severity": "high", "reachable": false, "fix_available": true},
        {"component": "pkg-gamma", "severity": "medium", "reachable": true, "fix_available": false},
        {"component": "pkg-delta", "severity": "low", "reachable": true, "fix_available": true}
      ]
  - path: input/policy.md
    content: |
      Priority P0: severity critical and reachable.
      Priority P1: severity high and fix_available true.
      Priority P2: severity medium and reachable.
      Priority P3: everything else.
      Recommended action for fix_available true is upgrade.
      Recommended action for fix_available false is mitigate and monitor.
---

## Prompt

Read `{attempt_root}/input/findings.json` and `{attempt_root}/input/policy.md`.

Create:

1. `{attempt_root}/output/risk_register.json`
2. `{attempt_root}/output/triage_brief.md`

`risk_register.json` must be a JSON array. Each item must include:

```json
{
  "component": "",
  "priority": "P0|P1|P2|P3",
  "action": "",
  "reason": ""
}
```

Rules:

- Sort items by priority from P0 to P3.
- Use `upgrade` as the action when a fix is available.
- Use `mitigate_and_monitor` as the action when no fix is available.
- `triage_brief.md` must name the P0 item and the no-fix item.

Only write results under `{attempt_root}/output`.

## Expected Behavior

The agent should convert security scanner findings into a prioritized, deterministic risk register with concise mitigation guidance.

## Grading Criteria

- [ ] Valid risk register JSON
- [ ] Correct priority assignment
- [ ] Correct action assignment
- [ ] Correct sorting
- [ ] Brief covers urgent and no-fix items

## Automated Checks

```python
def grade(transcript, workspace_path):
    import json
    import os

    scores = {
        "register_exists": 0.0,
        "brief_exists": 0.0,
        "priorities_correct": 0.0,
        "actions_correct": 0.0,
        "sorted_correctly": 0.0,
    }

    register_path = os.path.join(workspace_path, "output", "risk_register.json")
    brief_path = os.path.join(workspace_path, "output", "triage_brief.md")
    if os.path.exists(register_path):
        scores["register_exists"] = 1.0
    if os.path.exists(brief_path):
        scores["brief_exists"] = 1.0
    if not os.path.exists(register_path):
        return scores

    with open(register_path, "r", encoding="utf-8") as fp:
        items = json.load(fp)

    mapping = {item.get("component"): item for item in items if isinstance(item, dict)}
    expected_priorities = {
        "pkg-alpha": "P0",
        "pkg-beta": "P1",
        "pkg-gamma": "P2",
        "pkg-delta": "P3",
    }
    if all(mapping.get(component, {}).get("priority") == priority for component, priority in expected_priorities.items()):
        scores["priorities_correct"] = 1.0

    if (
        mapping.get("pkg-alpha", {}).get("action") == "upgrade"
        and mapping.get("pkg-beta", {}).get("action") == "upgrade"
        and mapping.get("pkg-gamma", {}).get("action") == "mitigate_and_monitor"
        and mapping.get("pkg-delta", {}).get("action") == "upgrade"
    ):
        scores["actions_correct"] = 1.0

    priorities = [item.get("priority") for item in items if isinstance(item, dict)]
    if priorities == ["P0", "P1", "P2", "P3"]:
        scores["sorted_correctly"] = 1.0

    return scores
```
