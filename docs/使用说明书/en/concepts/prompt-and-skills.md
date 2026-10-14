---
title: Prompts and Skills
summary: The system prompt shapes the agent's persona, and skills are its operating manuals for a class of task.
updated_at: 2026-10-13
---

An agent's behavior is decided by two things: its **system prompt** (who it is, how it thinks, what principles it holds) and its **skills** (the concrete method it follows for a class of task).

## The system prompt

The system prompt is the agent's working manual. It sets out the role it plays, the engineering principles it follows, how it uses tools, how it calls on skills, and what memory context it remembers.

**The system prompt is fixed when the session is created and stays frozen afterward.** This keeps the agent's behavior consistent within the same session.

## Skills

A skill is an operating manual for the agent. It bundles a full method, execution scripts, examples and resources, and the constraints and boundaries that apply. When it needs to, the agent reads the complete skill document.

A skill packages a method for a class of task, so similar tasks follow the same method and results stay consistent.

Skills come in three layers.

| Layer | Description | Editable |
| --- | --- | --- |
| Built-in | Shipped with the system | Read-only |
| Custom | Created by a user or administrator | Editable |
| External | Imported from outside | Depends on the source |

## Prompt template packs

wunder supports several prompt template packs: a Chinese pack (Chinese style by default), an English pack (English style by default), and custom packs tailored as needed.

To switch, open the agent settings, find "Prompt template", pick one, and save.

**When it takes effect**: a new conversation uses the new template while existing conversations are unchanged. Changing a template and seeing no change in the current conversation is expected; starting a new conversation picks it up.

To build a custom template, copy a built-in one, edit the sections (role, principles, additional notes, and so on), and save it to use.

## A rule that is easy to trip over

Resource paths inside a skill document are written as **placeholders** rather than hard-coded paths. A skill lives in a different actual location in each runtime form, so the system substitutes the correct path when the skill is used.

## Prompts, skills, and memory each cover their own ground

| | Responsibility |
| --- | --- |
| Prompt | Defines the long-lived, stable boundary of behavior |
| Skill | Defines the additional execution protocol for a class of task |
| Memory | Defines the long-term facts injected when a session initializes |

The three work together, but their responsibilities are not mixed.

## Working with templates and skills

A template has a clear role, concise principles (keep the important constraints rather than piling them on), no internal contradictions, and is tuned against real results.

Skills are managed on the "Tools" page: see installed skills, enable or disable them, create a skill for a particular working scenario, and share one with the team.

Combining prompt templates, skills, and models produces agents of different styles: a coding assistant, a copywriter, a data analyst, a translation expert.

## Common mistakes

- Changing a template does not affect the current conversation right away; it only affects new conversations.
- A skill is not just a documentation file; it is a runtime resource the agent can actively read.
- Memory does not rewrite the prompt every round; it is injected once at initialization.

## Further reading

- [Prompts and personalization](/docs/en/capabilities/prompt-templates/) — how to work with templates
- [Memory (core)](/docs/en/concepts/core-memory/) — the design principles of memory
- [Tool system](/docs/en/concepts/tools/) — how tools and skills relate
