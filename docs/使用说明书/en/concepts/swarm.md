---
title: Swarm Collaboration
summary: How a swarm actually works: how the queen bee decomposes tasks, how worker bees execute, how results merge, and how status is displayed.
---

# Swarm Collaboration

## How It Works

A swarm is Wunder's multi-agent collaboration mechanism. When a task is too complex for a single agent, the swarm turns collaboration into a first-class capability that is trackable, repeatable, and recoverable inside the system.

### Core Concepts

| Role | Description | Analogy |
|------|-------------|---------|
| **Queen bee** | The primary agent hosting the collaboration | Project manager |
| **Worker bee** | An existing agent scheduled to participate | Team member |
| **Mission** | One concrete collaboration task | Work assignment |
| **Task** | A concrete subtask assigned to a worker bee | To-do item |

### Workflow

```
User request: complex task that needs division of labor
         ↓
    Queen bee receives and analyzes
         ↓
    Decomposes into subtasks
         ↓
    ┌────┼────┐
    ↓    ↓    ↓
  Bee A  Bee B  Bee C
  gather analyze write
    ↓    ↓    ↓
    └────┼────┘
         ↓
    Queen bee merges results
         ↓
    Final deliverable
```

## Why It Matters

### Collaboration Benefits

- **Parallel execution**: multiple agents work simultaneously, which is more efficient
- **Specialized division of labor**: different agents have different strengths
- **Visible process**: users can see each worker bee's progress and trace
- **Reliable results**: worker results merge back into the queen bee instead of scattering

### Swarm vs Subagents

| | Swarm | Subagents |
|--|-------|-----------|
| **What is scheduled** | Existing agents | Temporary derived child sessions |
| **Threads** | Worker bees reuse their task threads by default | Derived within the current session |
| **Best for** | Complex division-of-labor collaboration | Lightweight temporary tasks |
| **Resources** | Agents must be created in advance | No extra preparation needed |

## Worker Bee Thread Convention

When a worker bee is assigned a task, by default it reuses the worker bee's task thread within the scope of the originating task. Why?

- **Clean context**: not disturbed by old conversations
- **Focus on the current task**: only sees what the queen bee dispatched
- **No cross-contamination**: different tasks do not interfere with each other

You can also explicitly reuse a worker bee's existing thread, or force a brand-new thread.

## In Practice

### Viewing Swarms in the UI

**Middle column list**:
- Shows all swarm tasks
- Swarms with running tasks get a breathing highlight
- Quickly see what is running

**Canvas view**:
- Visualizes the collaboration relationships
- Lines between queen bee and worker bees represent dispatch relationships
- Each node shows its current status

**Tool traces**:
- Each node shows which tools were called
- Shows the current step
- Traces remain available after completion

### Status Reference

| Status | Meaning |
|--------|---------|
| Running | The worker bee is executing its task |
| Completed | The worker bee finished its task |
| Failed | The worker bee hit an error |
| Waiting | Waiting to be scheduled |

### When to Use

**Good fit**:
- The task can be clearly split into subtasks
- Different subtasks need different specialties
- Parallelism improves efficiency

**Poor fit**:
- Simple tasks one agent can handle alone
- Just forking a small temporary errand

### Task Thread Protection

The UI blocks switching threads while an agent is running. This protection avoids:
- Leaving a running task thread before it finishes
- Session state and workflows getting out of sync

If you need a new thread, wait for the current run to finish or stop it first.

## Common Misconceptions

- **Is a swarm just parallel requests?** No. The point is the collaboration relationship and result merging.
- **More worker bees are always better?** No. Coarse or overly fine granularity both hurt efficiency.
- **Can worker bees see the queen bee's context?** No. Worker bees only see the dispatched task content.

## Further Reading

- [Swarm (Core)](/docs/en/concepts/core-swarm/) — design principles of the swarm
- [Agent Swarm Tool](/docs/en/tools/agent-swarm/) — tool usage
- [Subagent Control](/docs/en/tools/subagent-control/) — working with subagents
