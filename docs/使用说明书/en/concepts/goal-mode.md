---
title: Goal Mode
summary: Fields of a session-level persistent goal, the four phases and the continuation gate, the in-process arming flag, model-side operations, and the available entry points.
read_when:
  - You want an agent to push a long task forward without you sending a message every round
  - A goal stopped advancing and you need to know why
  - You need to know which goal actions the model may take, and in which round
source_docs:
  - crates/wunder-runtime/src/services/goal/types.rs
  - crates/wunder-runtime/src/services/goal/driver.rs
  - crates/wunder-runtime/src/services/goal/tools.rs
  - crates/wunder-runtime/src/services/goal/authority.rs
  - crates/wunder-runtime/src/api/chat_goal.rs
updated_at: 2026-10-16
---

Goal mode hangs a **goal** on a **session** and pairs it with a driver that lives only in the current process. Whenever the session goes idle, the driver submits the next continuation (a goal round), and each round spends one unit of the model-round budget; whether it keeps going or stops is decided by the budget, the phase, and whether the goal is blocked.

## Goal Fields

| Field | Meaning |
| --- | --- |
| Objective | What to do, up to 4,000 characters |
| Phase | One of `active`, `paused`, `blocked`, `complete` — these four and nothing else |
| Continuation-round budget (`max_goal_rounds`) | The most automatic continuations that may run; defaults to 256 when the goal is created |
| Started rounds (`rounds_started`) | Only ever increases; it is never reset backwards |
| Revision | Increments on every change; an update must carry the goal id and its exact current revision, and a mismatch is rejected (compare-and-set on revision) |
| Blocked reason | When blocked, a machine code (`blocked_code`) plus a plain-language explanation (`blocked_message`) |

## When It Applies

| Situation | What to do |
| --- | --- |
| Long tasks with a clear completion condition that the model should push forward over several rounds | Use goal mode |
| Something a single round of Q&A can resolve | Just send a message |
| Work that needs you in the decision loop often | Talk directly, and add the question panel when needed |
| You only want the current round to go deeper | An ordinary round is enough; a model round has no built-in one-step-one-answer limit |

## Phases

| Phase | Meaning | How it is entered | How it is left |
| --- | --- | --- | --- |
| `active` | Able to continue | Create a goal, or resume | `paused` / `blocked` / `complete` |
| `paused` | Sitting still; a person can resume at any time | A manual pause, or a continuation round that was interrupted | Resume |
| `blocked` | Automatic continuation stops | The model self-reports the same blocking condition once at least 3 consecutive continuation rounds have run; or the driver fails to enqueue (`queue-failed`) | Resume (after the blocking condition is resolved) |
| `complete` | Achieved | The model or a person marks it complete, and only when it is truly done | Reopen with a new goal |

Whether the next round starts depends on a single gate: **the phase is `active`, the goal is armed inside the process, and `rounds_started` has not reached the budget** — all three must hold to continue. A person's message always wins; a continuation round never competes with a person for the session.

## Budget Unit and Round Boundaries

- The budget is counted in **model rounds**, not tokens. The system does not track how many tokens a goal burns, there is no token budget, and there is no design in which the budget runs out and drops the goal into some restricted state.
- There is **no cooldown** between rounds. As soon as the session is idle, the driver immediately decides whether to send the next round.
- **"Blocked" does not mean "hard"**: difficulty, uncertainty, and unfinished work are not reasons to report blocked. The agent must name a concrete blocking condition that persists across rounds.

## Arming Is Not Persisted

Whether a goal is armed is an in-process flag and **is not stored in the database**. After a process restart, a session restore, or a fork, the driver will not pick up automatically even if the phase is still `active` — it has to be armed again with an explicit `update_goal` carrying `action=resume`. When asking why a goal stopped moving, first check whether it is still armed.

## Model-Side Operations

The model side operates on a goal through three built-in tools.

| Tool | Effect |
| --- | --- |
| `get_goal` | Reads the current goal, including the id and revision that `update_goal` requires; the authoritative value must be fetched before an update |
| `create_goal` | Infers and creates a goal from a person's direct request; the objective is required and the round budget is optional |
| `update_goal` | Edit / pause / resume / complete / report blocked (`action=blocked`); needs the goal id plus the current revision, and reporting blocked is rejected before the minimum round count is reached |

Authority does not come from anything the model writes: human entry points stamp `__source = "human"` onto the turn and the goal-round driver stamps `__goal_round`. Edit, pause, and resume are allowed only in a person's round; complete and report-blocked are additionally allowed in the goal's **own automatic continuation rounds**.

The Pupa (desktop) **does not expose these three tools for users to pick** (they are hidden built-in capabilities); the runtime injects them per session, and the Beehive (web) and the Helm (cli) reuse the same injection.

## Entry Points

| Entry | Location |
| --- | --- |
| API | `GET`/`PUT`/`DELETE /wunder/chat/sessions/{id}/goal` (user side; requires an ownership check) |
| Slash command | `/goal [<objective>\|clear\|edit <objective>\|pause\|resume]` |
| Beehive | A goal bar in the message area |
| Pupa | A goal banner above the input box, with inline editing |
| Helm | The `/goal` command |
| Bridge | **No goal UI** (the governance surface offers no goal operations) |

## Points Often Misread

- A goal does not stop on a token limit; it looks only at the round budget, the phase, and arming.
- After a service restart an active goal does not resume on its own; a resume action is needed to arm it again.
- The model cannot mark a goal blocked at will: the same blocking condition must persist across at least 3 continuation rounds.
- A goal is not "a tool named goal" but three tools — `get_goal`, `create_goal`, `update_goal`; the old single tool and the old injection marker no longer exist.
- The Bridge has no goal panel; use the Beehive, the Pupa, or the Helm.
- Clearing a goal does not undo work already done: messages, tool results, and file changes already stored are kept.
