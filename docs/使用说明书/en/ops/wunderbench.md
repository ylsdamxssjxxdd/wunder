---
title: WunderBench Model Evaluation
summary: Use the full benchmark suite to automatically measure how well a model completes real Wunder agent tasks, and use the results for regression checks, model selection, and benchmark extension.
read_when:
  - You need to evaluate model quality inside Wunder's real agent pipeline
  - You need to export full benchmark records with model logs, tool calls, and workspace artifacts
  - You plan to add or adjust WunderBench tasks
source_docs:
  - docs/API文档.md
  - config/benchmark/tasks
  - src/ops/benchmark
---

# WunderBench Model Evaluation

WunderBench is Wunder's built-in model evaluation system. It does not only ask the model questions. It reuses the real Wunder agent pipeline: prepares a workspace, runs the task, allows tool calls, records artifacts, grades the output, and produces an exportable evaluation record.

Use it to answer:

- Can this model reliably complete real Wunder tasks?
- Did a model, prompt, or tool-policy change cause a capability regression?
- Which task suites are weakest and should be investigated first?
- Is a failure caused by model reasoning, tool usage, workspace preparation, or judge scoring?

## Entry Point

Open **Debug / WunderBench** in the admin console.

The page has four main operations:

- **Run the full suite**: WunderBench runs every available task by default; quick, core, and full profile tiers are no longer separate choices.
- **Choose the tested model**: the model that actually performs the task.
- **Choose the judge model**: used only by `llm_judge` and `hybrid` tasks.
- **Export evaluation record**: download a JSON package with run details, attempts, task specs, and model logs.

Admin APIs are also available:

| Operation | API |
|-----------|-----|
| List profiles | `GET /wunder/admin/wunderbench/profiles` |
| List tasks | `GET /wunder/admin/wunderbench/tasks` |
| Start run | `POST /wunder/admin/wunderbench/start` |
| List runs | `GET /wunder/admin/wunderbench/runs` |
| Get run detail | `GET /wunder/admin/wunderbench/runs/{run_id}` |
| Export record | `GET /wunder/admin/wunderbench/runs/{run_id}/export` |
| Cancel run | `POST /wunder/admin/wunderbench/runs/{run_id}/cancel` |

## Evaluation Scope

WunderBench now exposes a single evaluation scope: `full`. It runs every available task so model comparison is consistent across runs.

| Scope | Purpose | Selection Rule | Recommended Runs |
|-------|---------|----------------|------------------|
| `full` | Model selection, release validation, and regression comparison | Runs every available task | 2 |

Compatibility notes:

- `/wunder/admin/wunderbench/profiles` returns only `full`.
- Old clients or scripts may still send `quick`, `core`, or `standard`; the backend normalizes those values to `full`.
- You can still pass `suite_ids` or `task_ids` to manually narrow the task set when investigating a suite or failed task.

## Tested Model and Judge Model

The **tested model** performs the task. It reads the prompt, calls tools, writes workspace files, and produces final outputs.

The **judge model** assists scoring. It does not execute the task and is only used by:

- `llm_judge`: primarily scored by rubric-based model judgment.
- `hybrid`: combines automated checks with judge scoring for semantic quality, completeness, and reasoning quality.

`automated` tasks do not require the judge model. For formal comparisons, use a stable and capable judge model that is reasonably independent from the tested model. For local smoke tests, using the same model is acceptable.

## Current Coverage

Built-in task specs live in `config/benchmark/tasks/*.md`; optional assets live in `config/benchmark/assets/`.

Current suites cover these areas:

| Suite | Main Capability | Current Focus |
|-------|-----------------|---------------|
| `workspace-core` | Workspace reads/writes, file inventory, config repair | Correct input reading, structured output, workspace boundary control |
| `coding-agent` | Code understanding, bug fixing, command validation | Locate defects, edit code, run checks, explain changes |
| `office-workflow` | Office writing, triage, response drafting | Understand constraints, extract priorities, produce usable text |
| `knowledge-memory` | Multi-document comparison, summarization, structured extraction | Extract changes, identify impact, produce concise conclusions |
| `data-analysis` | CSV metric calculation, threshold detection, structured insight | Aggregate tables, flag risk, and write concise summaries |
| `ops-observability` | Log anomaly analysis and runbook drafting | Parse logs, apply rules, and produce troubleshooting guidance |
| `devops-workflow` | CI configuration repair and pipeline constraints | Make minimal config fixes while preserving critical steps |
| `security-triage` | Dependency risk ranking and remediation guidance | Assign priorities and actions from a fixed policy |

The current benchmark is a practical early baseline for Wunder's core agent abilities. It is not a general model leaderboard. Future suites should add long-context tasks, multi-turn collaboration, browser operations, channel workflows, complex tool chains, and recovery-from-failure cases.

## Reading Results

The run overview summarizes the most important fields:

| Field | Meaning |
|-------|---------|
| `readiness` | Readiness label: `production_ready`, `usable`, `risky`, or `not_ready` |
| `overall_score` | Mean score across tasks |
| `reliability_score` | Pass-rate-oriented stability signal |
| `tool_success_score` | Tool result success rate |
| `stability_score` | Completion rate and repeated-run variance |
| `efficiency_score` | Runtime efficiency signal |
| `weakest_suites` | Task suites to investigate first |
| `top_failures` | Failed attempts that deserve immediate review |

Interpretation:

- `production_ready`: strong score, pass rate, and tool success rate; continue with stricter release validation.
- `usable`: usable with visible weaknesses; suitable for limited scenarios or continued observation.
- `risky`: unstable; review failed tasks before adoption.
- `not_ready`: not suitable for default model or production paths.

Do not look only at `overall_score`. A high overall score with a low `tool_success_score` usually means the answer may look plausible while tool or workspace behavior is risky.

## Attempts, Artifacts, and Logs

A run contains tasks, and each task can contain multiple attempts. An attempt is the smallest useful troubleshooting unit.

Each attempt records:

- Task, tested model, judge model, and elapsed time.
- Workspace-relative path, such as `benchmark/{run_id}/{task_id}/attempt_{attempt_no}`.
- Model transcript, tool calls, tool results, and final output.
- Automated score details, judge score details, and final score.
- Errors, artifact summaries, token usage, and speed statistics.

New WunderBench runs enable admin debug logging for benchmark threads. Model attempt monitor session ids look like:

```text
bench-{run_id}-{task_id}-{attempt_no}
```

Judge monitor session ids look like:

```text
bench-{run_id}-{task_id}-{attempt_no}-judge
```

These logs are included in exports so developers can inspect model requests, model outputs, tool calls, tool results, workspace updates, and runtime performance.

## Exporting Evaluation Records

Click **Export evaluation record** on the WunderBench page, or call:

```http
GET /wunder/admin/wunderbench/runs/{run_id}/export
```

The exported JSON is a replay package for one run:

| Field | Content |
|-------|---------|
| `run` | Run metadata, status, models, profile, scores, and scorecard |
| `task_aggregates` | Per-task aggregate scores, pass rates, variance, and lightweight `attempt_refs` |
| `attempts` | Attempt results, scores, artifact summaries, and transcripts |
| `task_specs` | The task specs used during that run |
| `attempt_logs` | Monitor logs plus lightweight `attempt_ref` for model and judge attempts |
| `diagnostics` | Export notes, missing-log hints, and compatibility notes |

Export when:

- A run fails and needs system-level debugging.
- You need a release baseline.
- You compare two models or two prompt versions.
- You suspect workspace preparation, tool calls, or judge scoring is wrong.

## Adding Tasks

Create a Markdown file under `config/benchmark/tasks/`. Each task contains YAML frontmatter plus fixed Markdown sections.

Basic structure:

````markdown
---
id: task_sample
name: Sample task
suite: workspace-core
category: filesystem
grading_type: automated
timeout_seconds: 180
runs_recommended: 2
difficulty: easy
required_tools:
  - read_file
  - write_file
tags:
  - filesystem
languages:
  - en
workspace_files:
  - path: input/source.txt
    content: |
      sample input
---

## Prompt

Complete the task inside `{attempt_root}`.

## Expected Behavior

Describe the expected result.

## Grading Criteria

- [ ] Key check one
- [ ] Key check two

## Automated Checks

```python
def grade(transcript, workspace_path):
    return {"check_name": 1.0}
```
````

Common frontmatter fields:

| Field | Description |
|-------|-------------|
| `id` | Globally unique task id; `task_` prefix is recommended |
| `name` | Display name |
| `suite` | Suite id for manual filtering and weak-suite aggregation |
| `category` | Task category |
| `grading_type` | `automated`, `llm_judge`, or `hybrid` |
| `timeout_seconds` | Attempt timeout; internally clamped to at least 30 seconds |
| `runs_recommended` | Recommended repeat count |
| `difficulty` | `easy`, `medium`, or `hard` |
| `required_tools` | Tools expected or likely needed |
| `tags` | Task tags |
| `languages` | Task language |
| `workspace_files` | Files prepared before the attempt starts |

`workspace_files` supports:

- `path + content`: write inline content directly.
- `source + dest`: copy an asset from `config/benchmark/assets/` into the attempt workspace.

## Grading Methods

WunderBench supports three grading methods:

| Type | Best For | Notes |
|------|----------|-------|
| `automated` | File existence, valid JSON, test pass/fail, structured correctness | Stable and repeatable; best baseline |
| `llm_judge` | Writing quality, summary quality, reasoning completeness, constraint following | Flexible but judge-model dependent |
| `hybrid` | Tasks with both hard checks and semantic quality checks | Recommended for complex tasks |

Automated grader contract:

- Function name must be `grade(transcript, workspace_path)`.
- Return `{check_name: 0.0~1.0}`.
- `workspace_path` points to the attempt workspace root.
- `transcript` contains the execution trace and can be used to inspect tool behavior.
- Smaller check items make failures easier to diagnose.

Judge scoring guidance:

- The rubric should define full credit, partial credit, and mandatory penalties.
- Do not ask the judge model to score things that scripts can check reliably.
- Keep the judge model stable for important baselines, otherwise historical scores are not directly comparable.

## Task Design Principles

Good WunderBench tasks should be:

- **Realistic**: close to work users expect Wunder agents to do.
- **Reproducible**: fixed inputs, stable expectations, no external real-time dependencies.
- **Gradable**: at least part of the result can be checked automatically.
- **Bounded**: explicitly require all reads and writes to stay inside `{attempt_root}`.
- **Diagnosable**: scoring items distinguish understanding failures, tool failures, format failures, and missing artifacts.
- **Cost-controlled**: avoid letting a few oversized tasks slow down full-suite runs.

Avoid tasks that:

- Depend on live news, current websites, or unstable external data.
- Only contain open-ended subjective judgment.
- Require human confirmation before completion.
- Hide important requirements outside the prompt.
- Let grading scripts read outside the attempt workspace.

## Troubleshooting

### The model says the directory is empty

Check:

- Whether `workspace_files` is declared correctly.
- Whether `source` assets exist under `config/benchmark/assets/`.
- Whether the prompt uses `{attempt_root}` and instructs the model to operate inside it.
- Whether the export shows successful workspace creation in `attempts[].artifacts`, `attempts[].transcript`, and `attempt_logs`.

If the export shows files were created but the model still says the directory is empty, the model likely used the wrong path or tool arguments. If the export also lacks files, inspect task assets and workspace preparation.

### Automated scores are all zero

Check:

- Whether outputs were written to the exact required paths.
- Whether JSON, Markdown, or code files are valid.
- Whether the grader paths match the prompt paths.
- Whether the grader assumes unavailable external dependencies.

### Judge scores vary too much

Check:

- Whether the rubric is specific enough.
- Whether hard checks were incorrectly delegated to the judge model.
- Whether the judge model is consistent with previous baselines.
- Whether the task needs a higher `runs_recommended`.

### Runs hang or take too long

Check:

- Whether `timeout_seconds` is reasonable.
- Whether the model repeatedly calls failing tools.
- Whether workspace files are too large.
- Whether the task asks for unnecessary long reasoning or broad search.

## Relation to Other Observability Tools

WunderBench measures task quality. It is not a stress test.

| Capability | Focus |
|------------|-------|
| Session monitoring | Online thread state, events, token usage, and tool calls |
| Tool statistics | Tool usage hotspots and success rate |
| Performance sampling | Single-request pipeline latency |
| Throughput stress testing | Concurrent load capacity |
| WunderBench | Model task quality and regression detection |

## Further Reading

- [Monitoring and WunderBench](/docs/en/ops/benchmark-and-observability/)
- [Admin Panels Index](/docs/en/reference/admin-panels/)
- [API Index](/docs/en/reference/api-index/)
