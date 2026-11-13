---
title: Memory
summary: Long-term material injected once as an index at thread creation and then frozen, the three fragment layers (L0/L1/L2) with core/working/peripheral tiers, and how memory divides the work with the workspace and the knowledge base.
updated_at: 2026-10-16
---

An agent needs long-term information — who the user is, what they prefer, what policies an organization has, what background a project has. Such material is injected once when the thread is created and stays frozen afterwards, never rewritten dynamically on every round.

Three things follow: cognitive boundaries stay stable while the thread runs, so it does not drift as the conversation goes on; the system prompt stays stable, which helps cache acceleration; and it can clearly answer what background this thread started from. On the user side this shows up as personal preferences taking effect automatically in new conversations and the agent being able to use long-term material in the workspace and knowledge base. On the administrator side it shows up as controllable injection rules, sensitive information not leaking, conversations of the same kind starting from the same background, and a thread's initialization background being traceable.

## Layered structure

Fragments are split into three layers by injection cost: **L0 title** (`title_l0`, the only body text injected into the system prompt), **L1 summary** (`summary_l1`, shown in lists and search hits), and **L2 content** (`content_l2`, returned only by `get`).

Fragments also carry a `tier`: `core` / `working` / `peripheral` (new fragments default to `working`). The tier affects ranking and retention weight, but never changes the "L0 index only" injection shape.

When a thread is created, the system writes the L0 index into the system prompt as the thread's cognitive base; the body text is pulled on demand through the memory-management tool (`list` / `search` / `get` / `add` / `update` / `remove` / `clear`). Semantic recall is currently disabled (inventory mode); `recall` is only an alias of `search`, not a separate channel.

## Structure

Memory uses structured fragments rather than one long free-text block. Fragments are found by lookup, replaced individually without affecting the rest, marked invalid when outdated (`superseded` / `invalidated`), and carry traceable version relationships (`supersedes_memory_id` / `superseded_by_memory_id`).

## Freezing rules

Once the system prompt is set it stays frozen, and later conversation never modifies it dynamically, which keeps thread behavior stable. Long-term memory is injected only once at thread creation; even if `search` (or the `recall` alias) fires during a run, its result is only supplementary reference and never rewrites the system prompt.

## Division of sources

Workspaces, knowledge bases, and memory can all supply material, but their responsibilities differ:

| Source | Character | Use |
| --- | --- | --- |
| Workspace | The scene of the current task | Holds the files being worked on |
| Knowledge base | A long-term document source | Retrieves policies, material, and reference documents |
| Memory | Structured accumulation | Remembers who the user is, their preferences, and past decisions |

## When updates take effect

When a memory update takes effect depends on the thread: a newly created thread applies the latest memory automatically, while an existing thread is unaffected and keeps the copy from when it was created. After a personal preference is updated, a new conversation is needed for it to take effect.

## Limits

Memory and context compression have different jobs: compression governs near-field information such as conversation history, while memory supplies far-field information such as long-term background. Compression does not touch the memory inside the system prompt.

More memory is not better. Injecting too much crowds the conversation's context space, introduces irrelevant information that disturbs judgment, and raises token consumption. The system therefore has selection and limiting mechanisms and does not inject without bound.

## Points not to mix up

Memory does not update automatically every round; it is injected only at initialization and does not change during a run. Retrieval through the memory-management tool is a dynamic supplement and does not rewrite the base. Workspace files are not the same as memory either — a workspace is the scene of the current task, while memory is long-term accumulation.
