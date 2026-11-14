---
title: Goal State
summary: The six `goal` actions and their required fields, revision-based concurrency, authority by turn origin, the user-only creation boundary, and how the common failures read.
updated_at: 2026-11-14
---

# Goal State

A goal pins one objective to a session so the agent keeps pushing it forward whenever the session goes idle. Users start goals; on the model side there is only the `goal` tool, used to read and advance a goal that already exists.

## Actions

| Action | What it does | Required fields |
| --- | --- | --- |
| `get` | Read the current goal, its phase, and its revision | `action` |
| `edit` | Change the objective or the continuation-round cap | `action`, `goal_id`, `revision`, `objective` |
| `pause` | Pause and stop auto-continuation | `action`, `goal_id`, `revision` |
| `resume` | Resume and re-arm | same as above |
| `complete` | Mark it complete | same as above |
| `blocked` | Report a blocker | same as above plus `blocked_reason` |

There is no create action. Only the user starts or replaces a goal — via `/goal`, the goal bar, or the goal API. If an older client sends a create request, the runtime rejects it and tells the model to hand that decision back to the user.

```json
{
  "action": "edit",
  "goal_id": "goal_20261114",
  "revision": 4,
  "objective": "Rebuild the tools section of the manual from the tool matrix and add the goal page",
  "max_goal_rounds": 12
}
```

## Revision concurrency

Every change bumps the revision. All mutating actions must send the `goal_id` and the exact `revision` that `get` returned; a mismatch is rejected, so auto-continuation rounds and human interventions never overwrite each other. Call `get` right before writing instead of reusing a revision remembered from an earlier round.

An `edit` objective is capped at 4000 characters. `max_goal_rounds` accepts 1 to 10000 and cannot drop below the rounds already started.

## Authority

What a turn may do depends on where it came from:

| Turn origin | Allowed actions |
| --- | --- |
| Direct human request | `get`, `edit`, `pause`, `resume`, `complete`, `blocked` |
| This goal's own auto-continuation round | `get`, `complete`, `blocked` |

So changing the objective, pausing, and resuming — decisions that belong to the human — only work in a human turn. An auto-continuation round can only wrap up or report a blocker.

## Failure reads

- Stale revision: call `get` for the current value and retry with it, not with the old arguments.
- `edit` / `pause` / `resume` inside an auto-continuation round: authority is insufficient; wait for the user.
- `blocked` rejected for too few rounds: the same blocking condition has to persist for at least 3 consecutive continuation rounds. Difficulty, uncertainty, or useful remaining work is not a blocker.
- A goal that stopped moving: check the phase and the armed state. Armed is in-process only, so after a restart or a session resume an `active` goal still needs one `resume` before it continues.

## Where this page stops

Phase, round budget, and the continuation gate are designed on [Goal State System](/docs/en/concepts/goal-mode/); the goal banner, the goal bar, and the `/goal` command are covered on each form's usage page. This page only covers how to call the tool.
