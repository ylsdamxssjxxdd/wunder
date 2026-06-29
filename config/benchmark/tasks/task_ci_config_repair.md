---
id: task_ci_config_repair
name: CI config repair
suite: devops-workflow
category: config_fix
grading_type: automated
timeout_seconds: 240
runs_recommended: 2
difficulty: medium
required_tools:
  - read_file
  - edit_file
  - write_file
tags:
  - ci
  - yaml
  - devops
languages:
  - en
workspace_files:
  - path: ci/pipeline.yml
    content: |
      name: sample-ci
      jobs:
        test:
          runs-on: ubuntu-latest
          steps:
            - uses: actions/checkout@v4
            - uses: actions/setup-python@v4
              with:
                python-version: "3.10"
            - name: Install
              run: pip install -r requirements.txt
            - name: Test
              run: pytest
        package:
          needs: build
          runs-on: ubuntu-latest
          steps:
            - uses: actions/checkout@v4
            - name: Package
              run: python -m build
  - path: input/requirements.md
    content: |
      The test job should use Python 3.11.
      The package job must depend on test, not on a missing build job.
      Add a cache step for pip before installing requirements.
      Keep the file as YAML-like text; do not create a replacement format.
---

## Prompt

Repair `{attempt_root}/ci/pipeline.yml` according to `{attempt_root}/input/requirements.md`.

Also create `{attempt_root}/output/repair_notes.md` with a short summary of what changed and why.

Rules:

- Do not rename jobs.
- Do not remove checkout, setup-python, install, test, or package steps.
- Use Python `3.11`.
- Change `package.needs` so it depends on `test`.
- Add one pip cache step before the install step.

## Expected Behavior

The agent should make a minimal CI configuration repair, preserve the intended pipeline structure, and summarize the repair.

## Grading Criteria

- [ ] Python version updated to 3.11
- [ ] Package job depends on test
- [ ] Pip cache step added before install
- [ ] Existing critical steps preserved
- [ ] Repair notes written

## Automated Checks

```python
def grade(transcript, workspace_path):
    import os

    scores = {
        "pipeline_exists": 0.0,
        "notes_exists": 0.0,
        "python_version_fixed": 0.0,
        "needs_fixed": 0.0,
        "cache_added": 0.0,
        "critical_steps_preserved": 0.0,
    }

    pipeline_path = os.path.join(workspace_path, "ci", "pipeline.yml")
    notes_path = os.path.join(workspace_path, "output", "repair_notes.md")
    if os.path.exists(pipeline_path):
        scores["pipeline_exists"] = 1.0
    if os.path.exists(notes_path):
        scores["notes_exists"] = 1.0
    if not os.path.exists(pipeline_path):
        return scores

    with open(pipeline_path, "r", encoding="utf-8") as fp:
        text = fp.read()
    lowered = text.lower()

    if 'python-version: "3.11"' in text or "python-version: '3.11'" in text or "python-version: 3.11" in text:
        scores["python_version_fixed"] = 1.0

    if "needs: test" in lowered:
        scores["needs_fixed"] = 1.0

    cache_index = lowered.find("cache")
    install_index = lowered.find("pip install -r requirements.txt")
    if cache_index >= 0 and install_index >= 0 and cache_index < install_index and "pip" in lowered[cache_index:install_index + 80]:
        scores["cache_added"] = 1.0

    required = [
        "actions/checkout@v4",
        "actions/setup-python",
        "pip install -r requirements.txt",
        "pytest",
        "python -m build",
    ]
    if all(item in lowered for item in required):
        scores["critical_steps_preserved"] = 1.0

    return scores
```
