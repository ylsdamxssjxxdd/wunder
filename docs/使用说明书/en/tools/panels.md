---
title: Plan Panel / Question Panel / Sessions Yield
summary: Aliases of the three interaction-control tools, their argument limits and normalized spellings, the replacement and turn-ending semantics that apply at call time, and their item caps.
updated_at: 2026-11-12
---

The three tools belong to the same interaction-control family. None of them produces a business side effect; they only change what the frontend shows or how the current turn ends.

| Canonical name | Aliases |
| --- | --- |
| `计划面板` | `update_plan` |
| `问询面板` | `question_panel`, `ask_panel` |
| `会话让出` | `sessions_yield`, `yield` |

The standalone "sleep and wait" tool has also been deleted, and waiting semantics are now carried by `会话让出`.

## Plan Panel

Use it to build and maintain an execution checklist when a task takes several steps and the user wants to follow progress. Work that finishes in one go does not need a plan.

One canonical shape only: `{"plan":[{"step":"a","status":"in_progress"},{"step":"b","status":"pending"}]}` — every step is an object carrying `step` and `status`, plus an optional `explanation`. The string list with `current` that used to be advertised alongside it is gone from the schema and the tool description (it is still readable); when both `current` and `status` arrive, `status` wins and the call no longer fails — one shape fewer means one class of retries fewer.

Call it at three fixed moments: when the work starts (one step `in_progress`), after every finished step with the same full list (mark the finished one `completed`, move `in_progress` to the next), and once more at the end so no half-finished progress is left on the panel. Updating only at the end means the panel lied to the user in between. The last call must mark **every** step `completed`: the plan capsule above the composer then checks off and stops moving, even when the user asks the next question.

A plan is a set of steps, 1 to 12 items, each a step plus a status; a step without text fails with its index instead of being dropped silently. Statuses are `pending` / `in_progress` / `completed`, and variants such as `in-progress`, `done`, `complete`, `进行中`, `已完成` are normalized. Step text can use `step`, or `title` / `content` / `text`; the list key can be `plan`, `steps`, `todos`, or `items`. You can attach one line describing the change to the plan.

The call replaces the whole plan rather than patching it. Only one step may be in progress at a time; marking two fails and names both.

An empty plan, or one whose steps are all dropped, reports an error. Shapes are tolerated, semantics are not. Shapes: a payload wrapped as one string (the `input` / `raw` wrappers common in text and XML protocols) or a `plan` list written as a string is unwrapped. When hand-written JSON inside that string drops a character — `[` followed by `step": …` instead of `{"step": …` — the repair pass restores it and the call succeeds, so it needs no resend. Semantics: unknown fields, unknown statuses, or two `in_progress` steps all fail immediately and name the location (for example `plan[1].status`) together with the allowed values. Silently ignoring an argument is worse than failing, because the caller believes it succeeded and carries the wrong state forward. A payload that still cannot be parsed reports the expected shape, an example, and the keys that arrived, so do not retry it verbatim.

Argument errors use the standard failure envelope: `ok:false`, a human-readable `error`, `data` carrying `field` / `received` / `allowed` / `example`, and `error_meta.code` set to `TOOL_PLAN_UNKNOWN_FIELD`, `TOOL_PLAN_UNKNOWN_STATUS`, `TOOL_PLAN_CONFLICT`, or `TOOL_PLAN_INVALID_JSON`. `TOOL_PLAN_INVALID_JSON` carries the parser's position (for example `expected `:` at line 1 column 15`) and the raw snippet that arrived: fix that bracket or quote, not the wrapper around it. `retryable` is true: fix the named field and retry, no need to guess a different shape.

The result is a model-facing status card, not user copy: it carries `completed` / `total` / `current_step` / `next_step`, and while steps remain `data.hint` holds the reminder to keep the panel current (it says so explicitly when no step is in_progress). The generic model-side projection drops the outer `summary`, so the reminder lives in `data`. The plan itself reaches the user through a frontend event and never enters model context, so the model must not assume it will remember the checklist next turn; the receipt is the source of truth.

## Question Panel

Use it when several reasonable routes exist and the user has to decide before work continues. Opening the panel hands the current turn to the user, so do not keep assuming an answer within the same turn.

It can ask 1 to 4 questions, each with 1 to 4 options; an option can be a label plus description plus a recommended flag, or just a string. Each question can be set to multiple choice, single choice by default. The old flat single-question form (question, options, and a multiple-choice flag at the top level) is still accepted. An option whose label contains "recommended", or that carries the recommended flag, is recognized as the recommended choice.

More than 4 questions keeps only the first 4; a question with no options is dropped entirely; if everything is dropped, it reports an error. Dropped questions never vanish silently: `dropped_questions` and the hint name the position, what is missing, and which fields were supplied. Options can also be one string separated by `|` or `，`.

The panel is sent in a "stay open" form, and the turn ends there: the closing text tells the user to answer in the panel, and the stop reason is recorded as the question panel: opening the panel ends the turn and waits for the user's next message.

## Sessions Yield

Use it when the turn has nothing left to do but you want to state explicitly that you are yielding and waiting for an external event, rather than hard-coding a closing line. It is not a substitute for a final reply, nor a timer. Waking on schedule is handled by [Schedule Task](/docs/en/tools/schedule-task/).

You can give one line of yield note to tell the user the current state; leaving it empty uses the default waiting text. The result state is "yielded", the turn's answer is set to that note, and the stop reason is recorded as a yield.

In desktop mode (Desktop) `会话让出` is listed as a hidden tool and is not exposed to the agent.

## Errors

All three use the shared failure shell. The most common failures are argument validation: an empty plan in the Plan Panel, no usable options in the Question Panel, and parse errors from a malformed argument structure.

## Boundaries

- Panels neither authenticate nor approve: they only handle presentation and turn control, and have nothing to do with the approval tier for writing files or running commands.
- The results of the plan and the question are deliberately thin, to save context; do not stuff detailed content into the note for the model to read back.
- When the frontend stream drops, panel events may not render, but the turn-ending semantics still take effect: after a question or a yield, do not expect the same turn to continue.
- Limits: at most 12 plan items, at most 4 questions, and at most 4 options per question; anything beyond is rejected or trimmed.
