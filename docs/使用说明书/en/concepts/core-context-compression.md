---
title: Context Compression
summary: Long conversations are not cut short when they grow long. The system trims history in graded levels, keeps the most useful information, and lets the conversation continue.
updated_at: 2026-10-13
---

## Why compression is needed

An agent's "memory" has a ceiling — the history a single conversation can carry cannot grow without bound, and the model's context window sets that limit. As a conversation grows longer, the total of history plus tool results can exceed the ceiling.

Context compression trims the history without interrupting the conversation, keeping the most useful information so the conversation keeps running.

Compression is not deletion. Deletion cuts content outright, regardless of importance; compression reorganizes content, keeping the key facts and trimming the redundant parts.

## How compression is graded

Compression escalates from gentle to aggressive only when necessary:

| Level | Strategy | Degree of loss |
| --- | --- | --- |
| Level 1 | Remove intermediate tool results | Very low; tool results can be fetched again |
| Level 2 | Summarize historical messages | Medium; keeps semantics, loses detail |
| Level 3 | Remove earlier conversation rounds | Higher; keeps only the most recent content |
| Level 4 | Keep only the core setup and the current request | Very high; emergency degradation |

## What it protects and what it leaves alone

Without compression, a long conversation ends in one of three ways: an outright error, a brute-force truncation, or manual control of conversation length. Compression is a system capability, so conversation length needs no manual management.

Compression protects three things first: the continuity of the conversation, so it is not interrupted for being too long; the facts genuinely needed, favoring information useful to the current task; and traceability, so every compression leaves a record of what it compressed and what it kept for administrators to inspect.

There are two bottom lines. First, compression can only trim and reorganize existing content; it never invents. Second, a thread's core setup (the system prompt) is frozen at creation, and compression acts only on the conversation history and tool results, so an agent never "forgets who it is" because of compression.

## Compression is inspectable

Every compression leaves a record that includes the strategy used, the context size before and after, and a rough assessment of the information lost. Administrators use it to judge whether compression has affected task quality.

## In practice

These situations tend to trigger compression: a very long conversation (dozens of rounds or more), a tool returning a large amount of content (reading a large file, many search results), and complex multi-step tasks.

To trigger it less often, open a new thread once a topic is done rather than keeping an old thread alive, keep prompts concise, and avoid pasting large repeated blocks; the `/compact` command triggers compression deliberately.

After compression, quality mostly does not drop noticeably, because the system favors what the current task needs. Only in extreme cases (a very long conversation stacked with a large amount of tool output) might you notice the agent recalling early details less precisely, and then a new conversation thread is the fix.

## Local reduction comes first

When automatic compression triggers, the system first sees whether earlier file reads, searches, and listing results can be shortened. The last 5 groups of tool results, error information, media, and results that still need pagination are protected. A shortened old result still carries its location, a head/tail preview, and an explicit omission notice, so the agent can read it again when needed.

If the local reduction has already freed enough space, no extra model call is made to produce a summary; if not, the summary flow runs. Manual compression and overflow recovery go straight to summarization. A summary outputs at most 2048 tokens by default, and if the model itself is configured with a smaller output limit, the smaller one wins.

Administrators can see the estimated savings of the local reduction in the progress records. The estimate is for diagnosis only; actual token consumption follows the model provider's report.

## Related pages

- [Boundary Handling](/docs/en/concepts/boundary-handling/) — system behavior under various boundary conditions
- [Tokens and Quota](/docs/en/concepts/quota-and-token-usage/) — how context usage is measured
- [Memory](/docs/en/concepts/core-memory/) — the relationship between long-term data and context compression
