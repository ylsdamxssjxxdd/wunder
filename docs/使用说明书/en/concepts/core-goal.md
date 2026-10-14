---
title: Goal
summary: The Goal core lets an agent enter goal mode and keep working on one task: three tools drive it, four phases, a default 256-round cap, and automatic blocking after 3 consecutive rounds without progress.
updated_at: 2026-10-13
---

# Goal

The Goal core lets an agent enter goal mode and keep working on one task. Attach a goal to a session and the agent carries on by itself, until it completes, is blocked, or exhausts the round budget.

## How it works

A goal is driven by three tools:

| Tool | Role |
| --- | --- |
| `get_goal` | Reads the current goal and its revision |
| `create_goal` | Creates a goal |
| `update_goal` | Edits, pauses, resumes, completes, or reports blocked |

A goal has only four phases: `active`, `paused`, `blocked`, `complete`.

Updates use CAS-on-revision: an update must carry the goal's current revision, and a mismatch is rejected. Started rounds only ever increase.

## Key numbers

| Item | Value |
| --- | --- |
| Default goal round cap | 256 |
| Automatic blocking after no progress | 3 rounds |
| Phase values | active / paused / blocked / complete |

## Where it is used

| Entry | Location |
| --- | --- |
| Interface | `/wunder/chat/sessions/{id}/goal` |
| Slash command | `/goal` |

## Boundaries and current limits

- There is no token budget and no cooldown between rounds. Whether a goal stops depends on three things only: the round budget, the phase, and whether it is armed.
- The **armed** state is **not persisted**: after a process restart or a session resume, even if the phase is still `active`, an explicit `update_goal action=resume` is needed to arm it again. When asking "why has the goal stopped moving", check first whether it is armed.
- The desktop form does not expose goal selection; the runtime injects it per session.
- The Bridge has no goal UI; goal operations happen in the Beehive, the Pupa, and the Helm.
