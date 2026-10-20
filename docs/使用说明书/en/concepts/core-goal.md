---
title: Goal
summary: The driver tools of goal mode, its four phases, the default 256-round cap, and CAS-on-revision updates.
updated_at: 2026-10-16
---

# Goal

A goal is attached to a session; the agent keeps working on it until the goal completes, becomes blocked, or exhausts the round budget.

## Driver tools

| Tool | Role |
| --- | --- |
| `goal` | Uses `action=get|create|edit|pause|resume|complete|blocked` to read, create, edit, pause, resume, complete, or report blocked |

## Phases

Four values: `active`, `paused`, `blocked`, `complete`.

Updates use CAS-on-revision: an update must carry the goal's current revision, and a mismatch is rejected. Started rounds only ever increase.

## Defaults

| Item | Value |
| --- | --- |
| Goal round cap | 256 |
| Automatic blocking after no progress | 3 rounds |

## Entry points

| Entry | Location |
| --- | --- |
| Interface | `/wunder/chat/sessions/{id}/goal` |
| Slash command | `/goal` |

## Limits

- There is no token budget and no cooldown between rounds. Whether a goal stops depends on three things only: the round budget, the phase, and whether it is armed.
- The **armed** state is **not persisted**: after a process restart or a session resume, even if the phase is still `active`, an explicit `goal action=resume` is needed to arm it again. When a goal stops moving, check armed first.
- The desktop form does not expose goal selection; the runtime injects it per session.
- The Bridge has no goal UI; goal operations happen in the Beehive, the Pupa, and the Helm.
