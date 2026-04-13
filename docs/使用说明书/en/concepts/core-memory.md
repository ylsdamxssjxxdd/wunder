---
title: Memory
summary: The Memory core explains why Wunder insists on "inject once during thread initialization, keep frozen afterward", separating long-term data utilization from thread cognitive stability.
read_when:
  - You are designing long-term memory, knowledge base, or data injection strategies
  - You want to understand the relationship between prompt freezing and memory injection
source_docs:
  - docs/总体设计.md
---

# Memory

Wunder allows long-term data to participate in execution, provided it does not undermine the thread's first-class reality. The focus of memory design is never "stuff in as much data as possible" but rather "how to let data enter a thread without causing the thread to drift."

![Memory diagram: during thread initialization, construct prompt and inject memory once; at runtime, supplement via recall without rewriting the foundation](/docs/assets/manual/core-memory.svg)

## Bottom Line

- Memory is not dynamically rewriting the prompt every turn; it is the long-term data foundation laid during thread initialization.
- Memory and recall must be treated separately: the former is the foundation, the latter is runtime retrieval.
- Once a thread starts running, the memory system must not continuously rewrite its cognitive boundaries.

## Why It Must Be a Core

Without a dedicated "memory" core, long-term data typically falls into two bad patterns:

- Appending to the prompt every turn, drifting further with each run.
- Writing recall results back as a permanent foundation, causing thread cognitive boundaries to spiral out of control.

Wunder isolates the memory core specifically to separate "how long-term facts enter a thread" from ordinary context concatenation.

## What This Core Actually Protects

- Thread stability: prevents prompts and long-term data from drifting during execution.
- Long-term data utilization quality: lets knowledge bases, workspace data, and memory fragments each serve their own purpose.
- Cache and review capability: can clearly answer "what foundation was this thread started on."

## Key Constraints

| Constraint | Purpose |
|------|------|
| Long-term memory is injected only once during thread initialization | Prevents continuous drift at runtime |
| `system prompt` is frozen once first established | Keeps thread cognitive foundation and cache stable |
| Recall is not equivalent to rewriting the foundation | Runtime data retrieval should not rewrite the thread's primary settings |
| Memory should be structured, not a long prose block | Facilitates matching, replacement, invalidation, and tracking |

## Design Highlights

### Highlight 1: The value of memory is "stable recall," not "remembering more"

For a long-running system, stable cognitive boundaries are more important than temporarily cramming in more data.

### Highlight 2: Initialization injection and runtime retrieval must be layered

A thread should start with a clear long-term background; if more data is needed later, it should be retrieved through recall or tool lookups rather than going back to rewrite the foundation.

### Highlight 3: Workspaces, knowledge bases, and memory fragments are not the same thing

They all provide data, but with different responsibilities:

- Workspaces are closer to the current task environment.
- Knowledge bases are closer to long-term document sources.
- Memory is closer to structured沉淀 of long-term facts.

## Common Misconceptions

- Long-term memory is not re-injected every turn.
- Memory is not a universal fallback; incorrect memory and excessive injection equally pollute execution.
- Knowledge bases, workspace files, and structured memory can all provide data, but their responsibilities are distinct.

## Boundaries with Other Cores

- Difference from [Context Compression](/docs/en/concepts/core-context-compression/): compression governs near-field history, memory governs far-field long-term data.
- Difference from [Tools](/docs/en/concepts/core-tools/): tools can expose recall capabilities, but the memory core defines timing and boundaries.
- Difference from [Agent Loop](/docs/en/concepts/core-agent-loop/): the loop defines how a thread runs, memory defines what it starts with.

## Further Reading

- [Core Overview](/docs/en/concepts/)
- [Long-term Memory](/docs/en/concepts/memory/)
- [Prompts & Skills](/docs/en/concepts/prompt-and-skills/)
- [Workspaces & Containers](/docs/en/concepts/workspaces/)
