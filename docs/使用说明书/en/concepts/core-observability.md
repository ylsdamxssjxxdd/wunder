---
title: Observability
summary: The observability core explains how Wunder splits "what happened, why, and how to review" into three distinct semantic layers: the fact layer, replay layer, and profile layer.
read_when:
  - You are working on troubleshooting, replay, evaluation, or admin profiling
  - You want to establish unified metrics and event standards
source_docs:
  - docs/总体设计.md
---

# Observability

If a system can only run but cannot explain itself, it becomes difficult to truly maintain. Wunder elevates observability to a core principle because troubleshooting, replay, profiling, and evaluation are themselves formal system capabilities, not auxiliary tools.

![Observability diagram: three-layer separation of fact layer, replay layer, and profile layer](/docs/assets/manual/core-observability.svg)

## Key Takeaways

- Observability is not "adding more logs", but structuring the system's explanatory capability into distinct layers.
- Facts, replay, and profiles must be separated; otherwise, frontend visualizations, admin statistics, and underlying raw events will pollute each other.
- Unified metric standards matter more than adding another monitoring dashboard.

## Why It Must Be a Core Principle

Without observability as a core principle, systems typically fall into two inefficient states:

- When problems occur, you can only guess—you know "something is wrong" but not "which step went wrong".
- There is plenty of data but the standards are chaotic; frontend, admin pages, logs, and stress test reports all speak different languages.

Wunder elevates observability to ensure that "explaining system behavior" becomes a first-class capability, not an ad-hoc emergency measure.

## What This Core Truly Protects

- Protects fact clarity: First know what actually happened.
- Protects review capability: Not only see the current state, but also reconstruct the process along the timeline.
- Protects governance efficiency: Admins see actionable profiles, not a pile of raw noise.

## Key Constraints

| Constraint | Purpose |
|------|------|
| Must separate fact layer, replay layer, and profile layer | Prevents monitoring, replay, and dashboards from polluting each other |
| Must distinguish requests, results, and observed results | Avoids mixing input, output, and tool observations into a single statistical metric |
| Metric standards must be unified | The same metric seen on different pages and channels must be alignable |
| Profiles cannot replace facts | What admin pages show is a curated perspective, not the underlying raw truth |

## Design Priorities

### Priority 1: Facts first, then interpretations, then profiles

A mature system should not directly infer underlying truth from admin charts, but should be able to trace from the fact layer all the way to the profile layer.

### Priority 2: Observability must serve two types of people

- For developers and operators: Answer "why it failed, where it got stuck, how to review".
- For administrators: Answer "where are the hotspots, where are the risks, what needs governance".

### Priority 3: benchmark, throughput, and monitor are not the same thing

They all belong to observability capabilities, but with different focuses:

- `monitor` looks at thread facts and runtime details.
- `throughput` looks at system load.
- `benchmark` looks at capability quality and regression.

## Common Misconceptions

- "Lots of logs" does not equal "good observability".
- What admin panels show are profiles, not necessarily the raw form of underlying facts.
- The more metrics you have without unified standards, the more distorted troubleshooting becomes.

## Boundaries with Other Cores

- Difference from [Realtime](/docs/en/concepts/core-realtime/): Realtime focuses on online synchronization, observability focuses on explanation and review.
- Difference from [Stability](/docs/en/concepts/core-stability/): Stability is responsible for keeping the system from failing, observability is responsible for understanding failures when they happen.
- Difference from [Multi-user Management](/docs/en/concepts/core-multi-user-management/): Multi-user management defines segmentation boundaries, observability forms profiles on those boundaries.

## Further Reading

- [Core Overview](/docs/en/concepts/)
- [Stream Events Reference](/docs/en/reference/stream-events/)
- [Performance and Observability](/docs/en/ops/benchmark-and-observability/)
- [Admin Panel Index](/docs/en/reference/admin-panels/)