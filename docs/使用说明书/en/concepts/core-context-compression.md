---
title: Context Compression
summary: The Context Compression core explains how Wunder actively manages context size during long sessions, making compression a traceable, observable, and reviewable formal operation.
read_when:
  - You are handling long sessions or oversized tool outputs
  - You want to understand why compression cannot be a simple summary
source_docs:
  - docs/总体设计.md
---

# Context Compression

Long sessions are not exceptions—they are inevitable in Wunder's real-world operation. Context compression is not a nice-to-have; it is the key mechanism that keeps threads running.

![Context compression diagram: full context passes through multi-stage compression and re-enters the window, preserving observational evidence](/docs/assets/manual/core-context-compression.svg)

## Bottom Line

- Compression is not "deleting some history"; it is the system actively reorganizing context.
- As long as the system supports long sessions, compression cannot be a temporary patch—it must be a formal core.
- Compression quality affects not just cost, but whether the thread can maintain semantic continuity.

## Why It Must Be a Core

Without context compression, long sessions eventually have only three bad outcomes:

- Directly exceed limits and fail.
- Brute truncation, causing thread memory loss.
- Manual length control, pushing system complexity onto users and integrators.

Wunder listing compression as a core is equivalent to admitting that "long session governance is a system responsibility."

## What This Core Actually Protects

- Thread continuity: preventing sessions from breaking due to context overload.
- Valid facts: preserving the information genuinely needed to continue execution.
- Reviewability: ensuring compression differences can be inspected, not becoming a black box.

## Key Constraints

| Constraint | Purpose |
|------|------|
| Compression cannot fabricate facts | Only reorganize existing content; cannot write facts that never existed |
| Compression must be traceable | Must be able to answer what was removed, what was kept, what was lost |
| Compression must be observable | Must leave events, levels, and results, not happen silently |
| Compression cannot break thread freeze constraints | Compressing history does not mean rewriting the thread foundation |

## Design Highlights

### Highlight 1: Compression is not truncation

Truncation just hard-cuts when content is too large; compression does information reorganization, aiming to keep the thread functional.

### Highlight 2: Compression timing matters

Compressing too early loses detail still needed; compressing too late lets the thread hit the window boundary directly. The real design focus is budget governance and progressive levels.

### Highlight 3: Tool observations are high-pressure zones

Long logs, large web pages, batch search results, and compound tool outputs all cause context to swell rapidly. So compression and tool design must be considered together.

## Common Misconceptions

- Compression is not equivalent to truncation. Truncation just discards content; compression requires preserving structured valid information.
- Compression is not a substitute for the memory system; long-term data should go through dedicated memory and knowledge pipelines.
- Just checking "whether limits were exceeded" is insufficient; also check whether compression changed task executability.

## Boundaries with Other Cores

- Difference from [Memory](/docs/en/concepts/core-memory/): memory governs long-term data entry; compression governs current thread history management.
- Difference from [Stability](/docs/en/concepts/core-stability/): stability cares about "how to recover after exceeding limits"; compression cares about "how to actively avoid hitting limits".
- Difference from [Observability](/docs/en/concepts/core-observability/): observability records what happened during compression; compression decides how to compress.

## Further Reading

- [Core Overview](/docs/en/concepts/)
- [Boundary Handling](/docs/en/concepts/boundary-handling/)
- [Token Accounts and Occupancy](/docs/en/concepts/quota-and-token-usage/)
- [Stream Events Reference](/docs/en/reference/stream-events/)