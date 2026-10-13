---
title: WunderBench Model Evaluation
summary: Use the full task bank to automatically verify how well a model completes real wunder agent tasks, and to support bank extension, regression detection, and model selection.
updated_at: 2026-10-13
---

WunderBench is wunder's built-in model capability evaluation system. It does more than ask the model questions: it reuses wunder's real agent execution pipeline — preparing a workspace, dispatching the task, letting the model call tools, recording artifacts, scoring automatically, and producing a complete exportable evaluation record.

Common uses: judging whether the current model can reliably complete real tasks inside wunder; checking whether a new model, new prompt, or new tool policy introduces a capability regression before release; identifying the weakest task suites and whether to optimize the model, the tools, or the system pipeline first; and locating whether a failure comes from model understanding, tool calls, workspace preparation, or judge scoring.

## Entry Point

Open the **Debug / WunderBench** page in the Bridge console.

The run overview shows only the status, total score, conclusion, evaluation target, and elapsed time. For full task execution, scoring, and tool-call diagnostics, open the corresponding single attempt in the execution record, or export the evaluation record.

The page offers six main operations:

- **Choose a question bank**: pick a built-in bank or an imported versioned bank; each run executes only the available tasks in the selected bank.
- **Import a question bank**: upload a `.zip` bank package from a trusted source. Banks containing automated grading scripts require explicit confirmation, because the scripts run on the local Python during evaluation.
- **Choose a preset agent**: pick the default agent or a configured preset agent and evaluate with its system prompt, tools, and skill preview.
- **Choose the tested model**: the model that actually performs the task; an explicit choice overrides the model configured by the preset agent.
- **Choose the judge model**: used only for subjective grading types.
- **Export the evaluation record**: download a JSON file containing the run process, each attempt, the task specs, and the model logs.

You can also call the Bridge APIs:

| Operation | API |
|-----------|-----|
| List preset agents | `GET /wunder/admin/wunderbench/preset_agents` |
| List question banks | `GET /wunder/admin/wunderbench/banks` |
| Import question bank | `POST /wunder/admin/wunderbench/banks/import` |
| List tasks | `GET /wunder/admin/wunderbench/tasks` |
| Start run | `POST /wunder/admin/wunderbench/start` |
| List runs | `GET /wunder/admin/wunderbench/runs` |
| Get run detail | `GET /wunder/admin/wunderbench/runs/{run_id}` |
| Export record | `GET /wunder/admin/wunderbench/runs/{run_id}/export` |
| Cancel run | `POST /wunder/admin/wunderbench/runs/{run_id}/cancel` |

## Evaluation Scope

The page does not offer scope tiers: once a bank is selected it runs every available task in it, so the same bank does not yield hard-to-compare results under different tiers.

| Scope | Purpose | Selection Rule | Recommended Runs |
|-------|---------|----------------|------------------|
| Full | Model selection, pre-release validation, and regression comparison | Run every available task | 2 |

Compatibility notes:

- The page always sends the full scope to the API.
- When an old client or script sends another scope value such as quick, core, or standard, the backend normalizes it to full.
- You can still filter by task suite or single task manually, to home in on a suite or re-check a failed task.

## Preset Agent, Tested Model, and Judge Model

After you choose a **preset agent**, WunderBench reads its current configuration at startup and writes the system prompt, tool and skill declarations, skill preview switch, preset model, preset identifier, and version into a run snapshot. Configuration changes made later do not affect a run in progress, and the exported record lets you review that run afterward.

A preset agent only supplies evaluation configuration; it is not bound to the evaluation thread as a specific user's actual agent. When no preset agent is selected, WunderBench falls back to model-only evaluation behavior.

Model selection follows this priority:

1. The tested model explicitly specified on the page or in the request.
2. The model configured by the selected preset agent.
3. The system default model.

The preset agent supplies its configured tool set; when a non-empty tool list is passed explicitly through the API, that field overrides the preset tools, which is useful for controlled tool-regression testing.

The **tested model** is the model that performs the task. It reads the prompt, calls tools, modifies the workspace, and produces the final output.

The **judge model** is a scoring aid. It does not replace automated checks and does not take part in task execution, and it is used only for these grading types:

- `llm_judge`: scoring relies mainly on the judge model against the rubric.
- `hybrid`: automated checks run first, then the judge model fills in dimensions the scripts cannot cover well, such as expression quality, completeness, and soundness of reasoning.

`automated` tasks depend only on script checks and do not need a judge model. To reduce bias, use a stable, capable, reasonably independent judge model for formal comparisons; for a local smoke test, using the same model temporarily is acceptable.

## Current Bank Coverage

Built-in tasks live under `config/benchmark/tasks/`, with assets under `config/benchmark/assets/`. Imported banks are stored under `config/benchmark/banks/{id}/{version}/`, with each version isolated so it does not mix into the built-in bank or other banks.

The built-in task suites cover these directions:

| Suite | Main Capability | Current Focus |
|-------|-----------------|---------------|
| `workspace-core` | Workspace reads/writes, file inventory, config repair | Correct input reading, structured output, workspace boundary control |
| `coding-agent` | Code understanding, defect repair, command validation | Locate problems, edit code, run checks, and explain the fixes |
| `office-workflow` | Office writing, information triage, reply drafting | Understand constraints, extract priorities, produce usable text |
| `knowledge-memory` | Multi-document comparison, summarization, structured extraction | Extract changes, identify impact, produce concise conclusions |
| `data-analysis` | CSV metric calculation, threshold detection, structured insight | Reliably aggregate tables, flag risk, and produce summaries |
| `ops-observability` | Log anomaly analysis, runbook generation | Parse logs, apply rules, and produce troubleshooting guidance |
| `devops-workflow` | CI config repair, pipeline constraint maintenance | Make minimal config fixes while preserving critical steps |
| `security-triage` | Dependency risk ranking and remediation guidance | Assign priorities and actions from a fixed policy |

The current bank leans toward wunder's basic agent abilities: files and workspaces, code repair, office workflows, and knowledge summarization. It works well as an early model-selection and regression baseline, but it is not yet a full general-purpose model leaderboard. Long-context, multi-turn collaboration, browser operations, external channels, complex tool chains, and failure-recovery tasks should be added over time.

## Question Bank Import Protocol

A bank package is a ZIP file whose root must contain `wunderbench.json`, organized as follows:

```text
wunderbench.json
tasks/
  task_example.md
assets/
  input.txt
```

A minimal `wunderbench.json` example:

```json
{
  "protocol": "wunderbench.question_bank",
  "schema_version": 1,
  "id": "sample-bank",
  "version": "1.0",
  "name": "Sample task bank",
  "tasks_path": "tasks",
  "assets_path": "assets"
}
```

The rules are:

- `id + version` is a non-overwritable version identity; revise a task by raising the version rather than replacing a historical file.
- Tasks under `tasks_path` follow this page's Markdown task format, so no industry-specific executor is needed.
- Files in `assets_path` can only be referenced relatively by a task's `workspace_files.source`, and task writes must stay inside the current attempt workspace.
- A ZIP may not contain path-traversal entries; import validates task ID uniqueness, asset existence, and package size.
- Before using a Python grading script with `automated` or `hybrid`, confirm the bank source is trusted; grading scripts are executable code.

A run record stores the bank identity, checksum, and task-spec snapshot. Even if a new version is imported or an old package is deleted later, the exported evaluation evidence can still be reviewed independently.

## Reading Results

The run overview gives a text summary; the fields worth watching are:

| Field | Meaning |
|-------|---------|
| `readiness` | Availability conclusion: `production_ready`, `usable`, `risky`, `not_ready` |
| `overall_score` | Mean score across all tasks |
| `reliability_score` | Stability on a pass-rate basis |
| `tool_success_score` | Tool-call result success rate |
| `stability_score` | Completion rate and cross-run variance |
| `efficiency_score` | An efficiency score tied to execution time |
| `weakest_suites` | The weakest task suites right now |
| `top_failures` | The failed attempts most worth re-checking first |

How to read the availability conclusion:

- `production_ready`: overall score, pass rate, and tool success rate are all high; ready for stricter release validation.
- `usable`: usable but with clear weak spots; suitable for continued observation or limited scenarios.
- `risky`: capability is unstable; review failed tasks first.
- `not_ready`: not suitable for the default model or production paths.

Do not look only at the total score. A high total score with a low tool success rate usually means the model may be "answering as if correct" while the tool pipeline or file operations are risky.

## Attempts, Artifacts, and Logs

A run contains multiple tasks, and each task can have multiple attempts. An attempt is the smallest unit for troubleshooting.

Each attempt records:

- The task used, the preset agent snapshot, the model, the judge model, and elapsed time.
- The relative path inside the workspace.
- The model transcript, tool calls, tool results, and final output.
- Automated scoring details, judge scoring details, and the final score.
- Errors, artifact summaries, and token/speed statistics.

Each new run enables admin debug logging for the evaluation thread. Logs for both model attempts and judge scoring come out with the export, covering model requests, model outputs, tool calls, tool results, workspace updates, and runtime performance.

## Exporting Evaluation Records

On the WunderBench page, click **Export evaluation record**, or call:

```http
GET /wunder/admin/wunderbench/runs/{run_id}/export
```

The exported JSON is a review package for one run, containing: the run's basic information, status, models, evaluation scope, total score, and scorecard; each task's aggregate score, pass rate, and variance; each attempt's results, scores, artifact summaries, and transcript; the task specs used at the time, so later bank changes don't prevent review; the corresponding monitoring logs and events; and export notes, missing-log hints, and compatibility notes.

Export in these situations:

- A model evaluation fails and needs to be handed to the system developers for analysis.
- A baseline is needed before release.
- Two models or two prompt versions are being compared and a complete evidence chain is needed.
- Workspace preparation, tool calls, or judge scoring are suspected to be wrong.

## Adding Tasks

To add a built-in task, create a Markdown file under `config/benchmark/tasks/`. To add a task to a distributable bank, create the same Markdown file under the bank's `tasks/`. Each task consists of YAML frontmatter and fixed Markdown sections.

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

Complete the task within `{attempt_root}`.

## Expected Behavior

Describe the result the model should produce.

## Grading Criteria

- [ ] First key checkpoint
- [ ] Second key checkpoint

## Automated Checks

```python
def grade(transcript, workspace_path):
    return {"check_name": 1.0}
```
````

Common frontmatter fields:

| Field | Description |
|-------|-------------|
| `id` | Globally unique task ID; a `task_` prefix is recommended |
| `name` | Display name on the page |
| `suite` | Task suite for manual filtering and weak-suite aggregation |
| `category` | Task category |
| `grading_type` | `automated`, `llm_judge`, or `hybrid` |
| `timeout_seconds` | Single-attempt timeout; treated as at least 30 seconds |
| `runs_recommended` | Recommended repeat count |
| `difficulty` | `easy`, `medium`, or `hard` |
| `required_tools` | Tools the model is expected to use or likely needs |
| `tags` | Task tags |
| `languages` | Task language |
| `workspace_files` | Files written into the workspace before the attempt starts |

`workspace_files` supports two forms:

- `path + content`: write inline file content directly.
- `source + dest`: copy an asset from `config/benchmark/assets/` into the attempt workspace.

## Grading Methods

WunderBench supports three grading types:

| Type | Best For | Notes |
|------|----------|-------|
| `automated` | Whether files are produced, whether JSON is valid, whether tests pass, whether structured results are correct | Stable and repeatable; good as the main baseline |
| `llm_judge` | Writing quality, summary quality, completeness of reasoning, whether the expression follows constraints | Flexible, but depends on the stability of the judge model |
| `hybrid` | Tasks with both script-checkable hard metrics and semantically judged soft metrics | Recommended for complex tasks |

Automated grading function conventions:

- The function name must be `grade(transcript, workspace_path)`.
- The return value is an object of the form `{check_item: 0.0~1.0}`.
- `workspace_path` points to the attempt workspace root.
- `transcript` contains the execution process and is good for checking whether the model called key tools or hit an exception.
- The finer the check items, the easier it is to locate a failure.

Judge scoring advice:

- The rubric should state clearly what counts as full credit, what counts as partial completion, and what must lose points.
- Do not hand content that scripts can judge to the judge model.
- Do not change the judge model frequently on important release baselines, or historical scores cannot be compared directly.

## Task Design Principles

Good WunderBench tasks should be:

- **Realistic**: close to the work wunder users would ask an agent to do.
- **Reproducible**: fixed inputs and stable expectations, with no reliance on external networks or real-time data.
- **Gradable**: at least part of the result can be verified by an automated script.
- **Clearly bounded**: require the model to read and write only inside `{attempt_root}`.
- **Diagnosable**: fine enough scoring items to distinguish understanding failures, tool failures, format failures, and missing artifacts.
- **Cost-controlled**: keep a single task from running too long, so a full run is not slowed down by a few big tasks.

Discouraged tasks:

- Rely on the current date, news, external web pages, or other unstable inputs.
- Contain only open-ended subjective evaluation, with no hard checkpoints.
- Require human confirmation to continue.
- Impose many implicit requirements on the model that the prompt does not state clearly.
- Let a grading script read paths outside the attempt workspace.

## Troubleshooting

### The model says the directory is empty

Check first:

- Whether the task's `workspace_files` is declared correctly.
- Whether the `source` asset exists under `config/benchmark/assets/`.
- Whether the prompt uses `{attempt_root}` and requires the model to operate only inside it.
- Whether the artifacts, transcript, and logs in the export show that the workspace was created successfully.

If the export shows the workspace files were created but the model still says the directory is empty, the model usually did not use the path or tool arguments correctly. If the export has no files either, check the task asset declarations and the workspace preparation pipeline.

### Automated scores are all zero

Check first:

- Whether the output was written to the path the task requires.
- Whether the JSON, Markdown, or code files are valid.
- Whether the automated grading script paths match the output paths in the prompt.
- Whether the grading script assumes an external dependency that does not exist.

### Judge scores vary widely

Check first:

- Whether the rubric is specific enough.
- Whether hard metrics that can be judged automatically were wrongly handed to the judge model.
- Whether the judge model is consistent with historical evaluations.
- Whether a single task needs a higher recommended repeat count to average out occasional variance across runs.

### Runs hang or take too long

Check first:

- Whether the single-task timeout is reasonable.
- Whether the model repeatedly calls failing tools.
- Whether the workspace files are too large.
- Whether the task requires unnecessary long reasoning or broad search.

## Relation to Other Observability Tools

WunderBench looks at task completion quality; it is not a stress test.

| Capability | Focus |
|------------|-------|
| Session monitoring | Current state of online threads, events, tokens, and tool calls |
| Tool statistics | Which tools are called most and how their success rate looks |
| Performance sampling | Single-request pipeline latency |
| Throughput stress testing | Concurrency capacity |
| WunderBench | Model task-completion quality and regression |

## Further Reading

- [Monitoring and WunderBench](/docs/en/ops/benchmark-and-observability/)
- [Admin Panels Guide](/docs/en/reference/admin-panels/)
- [API Reference](/docs/en/reference/api-index/)
