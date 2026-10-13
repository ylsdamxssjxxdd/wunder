---
title: Memory
summary: Long-term data is injected once when a thread is created and stays frozen afterwards; it has two layers, initialization injection and runtime recall.
updated_at: 2026-10-13
---

## What problem memory solves

An agent needs to "remember" long-term information — who the user is, what they prefer, what policies an organization has, what background a project has. If such material is cobbled together on the fly every time, the agent's behavior swings between good and bad.

wunder's approach is to inject it once when the thread is created and keep it frozen afterwards, never rewriting it dynamically on every round.

Three things follow: cognitive boundaries stay stable while the thread runs, so it does not drift as the conversation goes on; the system prompt stays stable, which helps cache acceleration; and it can clearly answer what background this thread started from.

## The two layers of memory

The first layer is thread-initialization injection. When a thread is created, the system writes the key long-term material into the system prompt, where it becomes the thread's cognitive base.

The second layer is runtime supplementary retrieval. If more material is needed during execution, the system retrieves it dynamically through recall rather than rewriting the system prompt.

## Who it helps

For ordinary users: personal preferences are remembered and take effect automatically in new conversations; the agent does not "forget" who the user is as a conversation goes on; and the agent can use long-term material in the workspace and knowledge base.

For administrators: memory-injection rules are controllable, which keeps sensitive information from leaking; conversations of the same kind start from the same background, so results are more predictable; and a thread's initialization background can be traced.

## What memory looks like

Memory uses structured fragments rather than one long free-text block. Fragments are found by lookup, replaced individually without affecting the rest, marked invalid when outdated, and carry traceable version relationships.

## A few rules that go with it

Once the system prompt is set it stays frozen, and later conversation never modifies it dynamically, which keeps thread behavior stable. Long-term memory is injected only once at thread creation; even if recall fires during a run, its result is only supplementary reference and never rewrites the system prompt.

Workspaces, knowledge bases, and memory can all supply material, but their responsibilities differ:

| Source | Character | Use |
| --- | --- | --- |
| Workspace | The scene of the current task | Holds the files being worked on |
| Knowledge base | A long-term document source | Retrieves policies, material, and reference documents |
| Memory | Structured accumulation | Remembers who the user is, their preferences, and past decisions |

## In practice

When a memory update takes effect depends on the thread: a newly created thread applies the latest memory automatically, while an existing thread is unaffected and keeps the copy from when it was created. So after you update a personal preference, a new conversation is needed for it to take effect.

Memory and context compression have different jobs: compression governs near-field information such as conversation history, while memory supplies far-field information such as long-term background. Compression does not touch the memory inside the system prompt.

More memory is not better. Injecting too much crowds the conversation's context space, introduces irrelevant information that disturbs judgment, and raises token consumption. The system therefore has intelligent selection and limiting mechanisms and does not inject without bound.

## A few clarifications

Memory does not update automatically every round; it is injected only at initialization and does not change during a run. Recall is a dynamic supplement and does not rewrite the base. Workspace files are not the same as memory either — a workspace is the scene of the current task, while memory is long-term accumulation.

## Related pages

- [Long-term Memory Details](/docs/en/concepts/memory/)
- [Prompts and Skills](/docs/en/concepts/prompt-and-skills/) — how the system prompt is organized
- [Context Compression](/docs/en/concepts/core-context-compression/) — governing near-field history
