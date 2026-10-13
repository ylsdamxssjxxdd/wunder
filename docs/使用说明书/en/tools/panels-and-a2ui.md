---
title: Plan Panel / Question Panel / Sessions Yield
summary: Three interaction-control tools: maintaining a visible execution plan, asking the user for a route before acting, and yielding the current turn to wait for an external event.
updated_at: 2026-10-13
---

The three tools belong to the same interaction-control family. None of them produces a business side effect; they only change what the frontend shows or how the current turn ends.

| Canonical name | Aliases |
| --- | --- |
| `计划面板` | `update_plan` |
| `问询面板` | `question_panel`, `ask_panel` |
| `会话让出` | `sessions_yield`, `yield` |

The old `a2ui` tool has been deleted and is not described here; the standalone "sleep and wait" tool has also been deleted, and waiting semantics are now carried by `会话让出`.

## Plan Panel

Use it to build and maintain an execution checklist when a task takes several steps and the user wants to follow progress. Work that finishes in one go does not need a plan.

A plan is a set of steps, 1 to 12 items, each a step plus a status; items with an empty step are dropped. Statuses are `pending` / `in_progress` / `completed`, and variants such as `in-progress`, `done`, and `complete` are normalized. You can attach one line describing the change to the plan.

The call replaces the whole plan rather than patching it. Only one step may be in progress at a time; if a second one appears, it is downgraded to pending.

An empty plan, or one whose steps are all dropped, reports an error. The result does not echo the plan content back; the plan reaches the user through a frontend event.

## Question Panel

Use it when several reasonable routes exist and the user has to decide before work continues. Opening the panel hands the current turn to the user, so do not keep assuming an answer within the same turn.

It can ask 1 to 4 questions, each with 1 to 4 options; an option can be a label plus description plus a recommended flag, or just a string. Each question can be set to multiple choice, single choice by default. The old flat single-question form (question, options, and a multiple-choice flag at the top level) is still accepted. An option whose label contains "recommended", or that carries the recommended flag, is recognized as the recommended choice.

When trimming and failing, more than 4 questions keeps only the first 4; a question with no options is dropped entirely; if everything is dropped, it reports an error.

The panel is sent in a "stay open" form, and the turn ends there: the closing text tells the user to answer in the panel, and the stop reason is recorded as the question panel. In other words, opening the panel ends the turn and waits for the user's next message.

## Sessions Yield

Use it when the turn has nothing left to do but you want to state explicitly that you are yielding and waiting for an external event, rather than hard-coding a closing line. It is not a substitute for a final reply, nor a timer.

You can give one line of yield note to tell the user the current state; leaving it empty uses the default waiting text.

The result state is "yielded", the turn's answer is set to that note, and the stop reason is recorded as a yield. Waking on schedule is handled by [Schedule Task](/docs/en/tools/schedule-task/).

In desktop mode (Honeycomb) `会话让出` is listed as a hidden tool and is not exposed to the agent.

## Diagnosing problems

All three use the shared failure shell. The most common failures are argument validation: an empty plan in the Plan Panel, no usable options in the Question Panel, and parse errors from a malformed argument structure.

## Boundaries

- Panels neither authenticate nor approve: they only handle presentation and turn control, and have nothing to do with the approval tier for writing files or running commands.
- The results of the plan and the question are deliberately thin, to save context; do not stuff detailed content into the note for the model to read back.
- When the frontend stream drops, panel events may not render, but the turn-ending semantics still take effect: after a question or a yield, do not expect the same turn to continue.
- Limits: at most 12 plan items, at most 4 questions, and at most 4 options per question; anything beyond is rejected or trimmed.

## Related pages

- How results enter model input and display: [Tools](/docs/en/tools/index/)
- Deriving and waiting: [Subagent Control](/docs/en/tools/subagent-control/)
