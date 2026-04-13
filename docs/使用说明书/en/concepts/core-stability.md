---
title: Stability
summary: The Stability core explains why Wunder must use structural governance to control fault propagation rather than relying on prompts, luck, and manual monitoring for stability.
read_when:
  - You are troubleshooting, stress-testing, or designing high-risk execution paths
  - You want to determine which layer a fault-tolerance capability should belong to
source_docs:
  - docs/总体设计.md
---

# Stability

Stability is not a safety net added at the end — it is a hard target that Wunder must be built around from the very beginning. The system faces not simple Q&A exchanges, but composite pipelines involving long sessions, high concurrency, multiple tools, and multiple entry points.

![Stability diagram: risk sources pass through isolation governance and recovery paths to prevent cascading failures and form a regressable system](/docs/assets/manual/core-stability.svg)

## Bottom Line

- Stability is not "fewer errors" — it is "when errors happen, they do not propagate, they can be recovered from, and they can be reviewed."
- The greatest danger in a long-running agent system is not a sporadic error, but an error that is not structurally contained.
- Relying solely on prompts, manual operations, or one-off hotfixes cannot sustain a truly stable system.

## Why It Must Be a Standalone Core

Without stability as a core concern, problems typically emerge at scale like this:

- A tool produces oversized output and drags down the entire context window.
- A network hiccup or model timeout breaks the entire execution chain.
- An anomaly at one entry point drags other entry points into an inconsistent state.

These problems cannot be solved by "retry a few more times" — they require structural governance.

## What This Core Really Protects

- Protects execution continuity — after an error, the system can continue rather than the entire chain becoming useless.
- Protects resource boundaries — high concurrency, long sessions, and large tool outputs must not grow unbounded.
- Protects system maintainability — similar problems must be categorizable, reproducible, and regression-testable.

## Key Constraints

| Constraint | Purpose |
|------|------|
| Stability depends on structural guarantees | Cannot rely only on prompts or manual intervention as a safety net |
| High-risk paths prioritize propagation control | Model, tools, network, and storage all need isolation strategies |
| Recovery must have a formal path | Timeouts, cancellations, disconnections, and failures must all have follow-up actions |
| Similar issues must be regression-verifiable | Avoid fixing once, forgetting, and repeating the same mistake |

## Design Highlights

### Highlight 1: Stability is first about "controlling propagation"

The real danger is not a single-point failure — it is a failure that takes down threads, tools, connections, queues, and data consistency all at once.

### Highlight 2: Recovery paths must be as formal as the main path

Resume-after-recovery, retries, redelivery, transaction rollback, outbox patterns, and atomic writes are not side-channel tricks — they are part of the main system.

### Highlight 3: Resource governance is part of stability

An oversized context, excessively long output, excessive concurrency, and queue backlog are fundamentally stability problems, not just performance problems.

## Common Misconceptions

- Just because the model produced a response does not mean the system is stable.
- Retries are not a cure-all — error isolation and resource governance are equally critical.
- A "sporadic issue that got fixed" without structured review is usually just temporarily quiet.

## Boundaries with Other Cores

- Distinction from [Context Compression](/docs/en/concepts/core-context-compression/): Compression manages window pressure; stability manages all high-risk paths.
- Distinction from [Realtime](/docs/en/concepts/core-realtime/): Realtime is about how state is delivered; stability is about whether state and execution remain consistent under abnormal conditions.
- Distinction from [Observability](/docs/en/concepts/core-observability/): Observability helps discover and review stability issues but does not directly replace stability design.

## Further Reading

- [Core Overview](/docs/en/concepts/)
- [Boundary Handling](/docs/en/concepts/boundary-handling/)
- [Troubleshooting](/docs/en/help/troubleshooting/)
- [Performance and Observability](/docs/en/ops/benchmark-and-observability/)
