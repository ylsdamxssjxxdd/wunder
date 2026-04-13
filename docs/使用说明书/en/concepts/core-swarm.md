---
title: Swarm
summary: The Swarm core explains how Wunder builds multi-agent collaboration into a formal system capability, rather than temporarily stuffing parallel tasks into a single large thread.
read_when:
  - You are designing multi-agent collaboration
  - You need to distinguish between swarms, sub-agents, and ordinary thread flows
source_docs:
  - docs/总体设计.md
---

# Swarm

A swarm is not simply "opening multiple chat windows" or "running several requests in parallel." It is Wunder's formal modeling of multi-agent collaboration relationships.

![Swarm structure diagram: queen bee main thread dispatches multiple worker bee threads, worker bee results merge back to the queen](/docs/assets/manual/core-swarm.svg)

## Key Takeaways

- The focus of swarms is not parallelism, but collaboration relationships, responsibility boundaries, and result merging.
- Worker bees must have clean threads, otherwise so-called collaboration is just copying and spreading dirty context.
- The queen bee is responsible for orchestration, worker bees for execution—this separation of responsibilities cannot be blurred.

## Why It Must Be Listed as a Core

Without the swarm as a core, multi-agent collaboration typically degenerates into two poor forms:

- Stuffing all roles into one thread, letting one model "play many people."
- Relying on peripheral scripts to temporarily dispatch requests, but with no parent-child relationships, state synchronization, or result merging.

Neither approach can stably express "who is doing what, who is responsible for what, and how results return to the main flow."

## What This Core Truly Protects

- Protects collaboration boundaries: Queen and worker responsibilities are separated, avoiding mutual contamination of context and accountability.
- Protects result merging: Worker bee results must re-enter the queen bee's main flow, not scattered externally.
- Protects process visibility: Collaboration is not a black-box batch process, but a continuously observable execution structure.

## Key Constraints

| Constraint | Purpose |
|------|------|
| Queen only orchestrates, doesn't swallow all execution | Keeps task decomposition and result merging clear |
| Worker bees create new threads by default | Keeps context clean, doesn't bring in old task residue |
| Collaboration relationships must remain continuously visible | Provides basis for state sync, workflows, and retrospectives |
| Results ultimately return to the main thread | Ensures the primary agent's first-class reality state is not lost |

## Design Focus

### Focus One: Swarms Manage "Existing Agents"

Swarm emphasizes dispatching worker bees that already exist. It's not temporarily creating an execution fragment, but calling existing roles into collaboration.

### Focus Two: Sub-agents and Swarms Are Not the Same Thing

Sub-agents are more like temporary forks of the current thread; swarms are more like the current agent dispatching other formal worker bees. Both can divide work, but the system semantics differ.

### Focus Three: Collaboration Quality Depends on Clear Boundaries, Not Worker Count

More worker bees, if decomposition is unclear, merging is unclear, and state is invisible—collaboration becomes even messier.

## Common Misconceptions

- Sub-agents are not equivalent to swarms. Swarms emphasize orchestration semantics, node states, and result merging.
- "Parallel" does not equal "efficient." Decomposition that's too coarse or too fine will reduce returns.
- Worker bee threads must be clean, otherwise context contamination directly weakens collaboration quality.

## Boundaries with Other Cores

- Difference from [Agent Loop](/docs/en/concepts/core-agent-loop/): Agent loop is single-threaded main flow, swarm is multi-threaded collaboration structure.
- Difference from [Realtime](/docs/en/concepts/core-realtime/): Realtime solves how collaboration states sync externally, swarm solves how collaboration relationships are modeled.
- Difference from [Stability](/docs/en/concepts/core-stability/): Stability controls the spread of worker bee failures, swarm defines why worker bees exist.

## Further Reading

- [Core Overview](/docs/en/concepts/)
- [Swarm Collaboration](/docs/en/concepts/swarm/)
- [Sub-agent Control](/docs/en/tools/subagent-control/)
- [Agent Swarm](/docs/en/tools/agent-swarm/)