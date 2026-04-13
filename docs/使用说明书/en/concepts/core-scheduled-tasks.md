---
title: Scheduled Tasks
summary: The Scheduled Tasks core explains why Wunder treats time-driven background tasks as a first-class system capability rather than leaving them to ad-hoc external scripts.
read_when:
  - You are designing inspection, reminders, background maintenance, or delayed execution tasks
  - You want to distinguish online threads from system-level async tasks
source_docs:
  - docs/总体设计.md
---

# Scheduled Tasks

Scheduled tasks let Wunder do more than just respond to incoming requests — they allow the system to proactively execute governance and business actions along the system timeline. What they solve is how time-driven tasks become part of a unified system capability, not how to write an external cron script.

![Scheduled Tasks diagram: scheduling rules enter the scheduler, then dispatch to background execution and execution records](/docs/assets/manual/core-scheduled-tasks.svg)

## Bottom Line

- Scheduled tasks are not an upgraded `sleep`; they are a system-level async scheduling capability.
- Time itself is a system input, so scheduling, execution, recording, and failure recovery must all be under formal governance.
- Any task that runs long-term or affects user or system state should not live in an opaque external script.

## Why It Must Be a Standalone Core

Without scheduled tasks as a core capability, periodic behaviors in the system typically scatter to:

- External cron scripts.
- Frontend polling timers.
- A manual trigger button used by some administrator.

The shared problem: triggering, execution, failure handling, tracing, and permission boundaries are no longer unified, and background tasks gradually spin out of control.

## What This Core Really Protects

- Protects scheduling semantics — there must be clear rules for why a task starts at a given time.
- Protects execution boundaries — background tasks must not be conflated with online session semantics.
- Protects traceability — every planned execution must have a status, a record, and a failure reason.

## Key Constraints

| Constraint | Purpose |
|------|------|
| Must be separated from online thread semantics | Avoids mixing background scheduling with live sessions |
| Execution records must be traceable | Answers "did it run, why did it fail, when is the next run" |
| Failure retry must have a strategy | Prevents a single transient failure from disabling a task permanently |
| Periodic tasks must have boundaries | Avoids unbounded accumulation and resource contention |

## Design Highlights

### Highlight 1: Bring "time" into the system, not leave it outside

Once the system needs scheduled inspections, scheduled reminders, or scheduled maintenance, time has become part of the business. It should therefore be governed by the system.

### Highlight 2: Scheduled tasks are a background execution chain, not a synchronous wait chain

`sleep` is more like a brief pause within the current flow; a scheduled task is more like "the system promises to do something at a future point in time."

### Highlight 3: Scheduling capability must natively carry execution records

A scheduling system without execution history, next-run time, and last status is essentially unusable in operations.

## Common Misconceptions

- `sleep` is not a scheduled task — the former is about synchronous waiting, the latter about async scheduling.
- Scheduled tasks are not a script escape hatch outside the main system — they are a formal governance capability.
- The more automated periodic execution becomes, the more it needs clear records and fallback governance.

## Boundaries with Other Cores

- Distinction from [Agent Loop](/docs/en/concepts/core-agent-loop/): The agent loop handles "how the current turn runs"; scheduled tasks handle "when to start a future run."
- Distinction from [Stability](/docs/en/concepts/core-stability/): Stability handles failure recovery; scheduled tasks give periodic execution a formal entry point.
- Distinction from [Observability](/docs/en/concepts/core-observability/): Observability records the results of scheduled executions; scheduled tasks define the schedules themselves.

## Further Reading

- [Core Overview](/docs/en/concepts/)
- [Scheduled Tasks](/docs/en/tools/schedule-task/)
- [Deployment and Operations](/docs/en/ops/deployment/)
- [Operations Overview](/docs/en/ops/)
