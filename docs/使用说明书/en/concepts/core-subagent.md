---
title: Subagents
summary: The Subagents core forks a subtask inside a single thread for an independent executor to complete and return to the parent; the only collaborative form is the subagent, and swarm orchestration has been removed from the engine and the protocol layer.
updated_at: 2026-10-13
---

# Subagents

The Subagents core forks a subtask inside a single thread: the parent thread hands one subtask to an independent executor, which runs it and passes the result back to the parent thread.

## How it works

`子智能体控制` (Subagent Control) provides 11 actions:

| Action | Action | Action |
| --- | --- | --- |
| `spawn` | `batch_spawn` | `wait` |
| `interrupt` | `close` | `resume` |
| `report` | `send` | `status` |
| `list` | `history` | |

Parent–child round references are encoded as `user_round` + `model_round`, and an executor's rounds hang under that pair of parent round references. The sub-session's user message is hidden, and it wakes with auto-wake.

## Boundaries and current limits

- The only collaborative form is the **subagent**. Swarm orchestration (swarm tools, hives, orchestration mode, team runs, swarm packs) has been removed from the engine and the protocol layer; it is not waiting to be implemented, nor hidden behind a switch.
- Swarm leftovers remain in TUI prompts, front-end i18n labels, and compatibility shims, and empty directories await cleanup. Those leftovers do not mean the capability exists.
