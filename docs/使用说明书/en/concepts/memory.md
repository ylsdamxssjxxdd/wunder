---
title: Long-term Memory
summary: wunder's long-term memory design revolves around thread freezing, one-time injection, and structured memory fragments.
read_when:
  - You need to understand why long-term memory emphasizes freezing and one-time injection
  - You need to troubleshoot the relationship between thread memory and the system prompt
source_docs:
  - docs/设计文档/01-系统总体设计.md
  - docs/API文档.md
---

# Long-term Memory

wunder's long-term memory design is not "append more text to the system prompt every round."

It does the opposite. It treats thread stability as the primary concern.

## Core rules

The current system enforces two mandatory constraints:

1. once an agent thread's system prompt is first determined, it must remain frozen
2. long-term memory may only be injected once during thread initialization

Later rounds may not repeatedly rewrite the thread's system prompt.

## Why it is designed this way

The reasons are direct:

- to preserve prompt caching
- to reduce context drift
- to keep thread behavior more stable
- to make it traceable which prompt a thread actually ran on

## What memory looks like

Long-term memory currently uses structured memory fragments.

It is not treated as one large free-form paragraph. Instead, it is organized in a form better suited to:

- matching
- replacement
- invalidation
- version tracking

## Memory and recall

Part of memory is injected when the thread is initialized.

If that initial injection is not enough, the orchestration and tool layers can still use recall-style mechanisms to retrieve more material, rather than repeatedly rewriting the thread system prompt.

## Memory and context compression

Even after compression, the system still has to decide whether memory is sufficient.

So the right distinction is:

- the frozen initial injection at thread creation
- the later recall that supplements context after compression

These are related, but they are not the same mechanism.

## The easiest mistakes to make here

- treating long-term memory as a dynamic per-round system prompt
- treating recall results as permanent writes back into the thread prompt
- treating token occupancy as a total billing number

## Further reading

- [Sessions and Rounds](/docs/en/concepts/sessions-and-rounds/)
- [Architecture](/docs/en/concepts/architecture/)
- [FAQ](/docs/en/help/faq/)
