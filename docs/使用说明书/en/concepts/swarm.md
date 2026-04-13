---
title: Swarm Collaboration
summary: wunder's swarm capability is for multi-agent collaboration, centered on queen orchestration, worker execution, and subagents only when needed.
read_when:
  - You need to understand wunder's multi-agent model
  - You need to distinguish queens, workers, and subagents
source_docs:
  - docs/API文档.md
  - frontend/src/components/beeroom/canvas/swarmCanvasModel.ts
  - frontend/src/components/beeroom/canvas/BeeroomSwarmNodeCard.vue
updated_at: 2026-04-10
---

# Swarm Collaboration

In wunder, a swarm is not just "open several chat windows." It is a formal multi-agent collaboration structure.

## What problem a swarm solves

A single agent is good at completing one task independently, but many tasks naturally need to be split, for example:

- one agent collects information
- one agent generates the output
- one agent reviews it
- one agent packages the final delivery

The swarm turns that collaboration pattern into a first-class system capability that can be traced, repeated, and recovered.

## First distinguish the three roles

### Queen

The queen is the main agent currently coordinating the collaboration.

It is responsible for:

- breaking down the task
- dispatching work to workers
- combining results
- deciding whether collaboration should continue

### Worker

Workers are other already-existing agents inside the swarm.

They execute the concrete tasks that are assigned to them.

### Subagent

A subagent is not itself a swarm member. It is a temporary child run created by a queen or worker during execution.

So remember:

- a worker is an **existing agent**
- a subagent is a **temporary derived run**

## Difference from subagent control

Both support collaboration, but their boundaries differ:

- swarm: dispatch existing agents
- subagent control: derive temporary child runs from the current session

If the goal is "bring in other agents that already exist," prefer the swarm.

If the goal is "fork a small temporary child task from the current session," prefer subagent control.

## Why workers start in a new thread by default

The current swarm system has one very important convention:

- when a worker receives a task, it starts in a new thread by default
- that new thread becomes the worker's new main thread

The purpose is to keep the worker's context clean and avoid dragging dirty history from an older conversation directly into the new assignment.

By default, the system reuses the worker's current main thread, creating and binding one first when needed. You can still pass `threadStrategy=main_thread` (or `reuseMainThread=true`) to make that intent explicit; `threadStrategy=fresh_main_thread` forces a clean new thread instead. Only an explicit `sessionKey` in `send` / `batch_send` pins the run to a specific existing thread.

## Why the main thread matters

In wunder, an agent's main thread is its first-class runtime reality.

This means:

- new tasks should land on the main thread first
- once a worker switches to a new main thread, later collaboration continues around that new thread

This is also why the frontend protects thread switching very strictly while a run is active.

## How the frontend displays swarm state right now

The swarm page currently exposes several stable state signals:

- as long as a swarm still has running missions, its item in the middle column keeps a pulsing highlight
- running nodes on the canvas show pulsing borders or active highlights
- the workflow area for worker and subagent nodes follows the latest progress automatically

So the user now sees not just who is busy, but how far each node has progressed.

## What the workflow area emphasizes now

The workflow area in the swarm canvas now focuses primarily on tool traces.

In particular:

- queen nodes preserve the real tool workflow instead of collapsing to a summary
- worker nodes keep showing their own tool steps
- subagent nodes keep their tool traces even after completion instead of only showing a terminal state label

In other words, the workflow area now prioritizes:

- which tools were called
- which step the node is currently on
- which nodes are still active

rather than just piling up status labels, session IDs, or summaries.

## When to use a swarm

Good fit:

- research, writing, and review in parallel
- assigning different roles to different aspects of the same problem
- dividing labor across multiple existing agents

Poor fit:

- small tasks that one agent can finish alone
- a very small temporary child task that only needs a quick fork

## Further reading

- [Agent Swarm](/docs/en/tools/agent-swarm/)
- [Subagent Control](/docs/en/tools/subagent-control/)
- [Thread Control](/docs/en/tools/thread-control/)
