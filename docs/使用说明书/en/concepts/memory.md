---
title: Long-term Memory
summary: How the agent "remembers" your preferences, background, and long-term material—structured memory is injected once when a conversation starts, then supplemented by recall as it runs.
updated_at: 2026-10-13
---

wunder's memory holds to one principle: **stability over flexibility**.

## Three core rules

1. **Injected once when the session is created**: long-term memory is written into the system prompt once, when the conversation starts.
2. **Frozen while running**: later turns do not change the system prompt.
3. **Supplemented by recall when needed**: while running, recall adds material, but it does not rewrite the foundation.

The reason it is not updated every round: if the system prompt were rewritten each round, the agent's behavior would be unstable (drifting as the conversation goes on), cache hits would fall (making it slower), and problems would be hard to trace (you could not say what background it was working from at the time).

**One injection plus freezing equals stable behavior plus a traceable background.**

## What memory looks like

Memory is made of **structured fragments**, not one long free-form passage. A fragment is found by lookup, replaced without affecting the rest, marked invalid when outdated, and carries traceable version relationships.

## What memory can hold

- **Who you are**: preferences and habits, work background, past decisions.
- **Organizational knowledge**: team rules and standards, project background, domain expertise.
- **Personalization**: adjusting the answer style to your preferences, referring to your past context, and giving targeted suggestions.

## Initialization injection vs runtime recall

| | Initialization injection | Runtime recall |
| --- | --- | --- |
| Timing | When the session is created | During the conversation |
| How | Written into the system prompt | Added as a tool result |
| Effect | Becomes the session's cognitive foundation | Does not rewrite the foundation |
| Scope | The life of the whole session | The current round |

## Managing memory

- **View**: see the current memory in the agent settings.
- **Add**: tell the agent "remember this preference," or add it by hand in settings.
- **Delete**: remove fragments that are outdated or unneeded.
- **Update**: add a new fragment to replace an old one.

When it takes effect: a **new conversation** uses the latest memory at once; an **existing conversation** is unaffected and keeps using the memory from when it was created. So after updating memory, you need to start a new conversation for it to apply.

## Memory and the knowledge base are not the same

| | Memory | Knowledge base |
| --- | --- | --- |
| Content | Structured personal / organizational facts | Documents, material, and standards |
| When used | Injected at initialization | Retrieved while running |
| Update frequency | Relatively stable | Can be updated often |
| Good for | Preferences, background, decision records | Reference material, technical documents |

## Easy things to get wrong

- Memory is not updated every round; it is injected once at initialization.
- The results of recall are not written back into the system prompt; they are only a temporary supplement.
- More memory is not better: too much crowds the context and can interfere with judgment.

## Further reading

- [Memory (core)](/docs/en/concepts/core-memory/) — the design principles of memory
- [Prompts and Skills](/docs/en/concepts/prompt-and-skills/) — how the system prompt is organized
- [Workspaces and Working Directory](/docs/en/concepts/workspaces/) — workspace management
