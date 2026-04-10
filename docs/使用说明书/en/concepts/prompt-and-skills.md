---
title: Prompts and Skills
summary: wunder treats system prompts and skills as governed runtime objects rather than temporary text scattered across the codebase.
read_when:
  - You need to understand why a thread's system prompt is frozen
  - You need to understand the relationship between Skills, template packs, and `skill_call`
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
  - docs/API文档.md
  - config/wunder-example.yaml
---

# Prompts and Skills

In wunder, prompts and skills are not optional annotations. They are part of the main execution chain.

## First separate the two categories

### System prompts

These define how the model should understand itself, the tool protocol, engineering constraints, and memory context inside the current thread.

### Skills

These define what additional operating manuals, resource paths, and execution conventions the model can read in the current session.

Both affect model behavior, but they serve different roles.

## Why the thread prompt must be frozen

wunder currently enforces one especially important rule:

- a thread's system prompt must be frozen after it is first determined

This is not conservative for its own sake. It is a stability requirement:

- it prevents a thread from drifting as the conversation grows
- it prevents long-term memory from repeatedly invalidating prompt caching
- it reduces personality inconsistency across long sessions
- it makes failures easier to reproduce and diagnose

So keep this straight:

- a new thread builds a new system prompt
- an existing thread is not rewritten just because the template pack changed

## How prompt template packs are organized

The current prompt system is organized in segmented files such as:

- `role`
- `engineering`
- `tools_protocol`
- `skills_protocol`
- `memory`
- `extra`

The system supports multiple template packs, and `prompt_templates.active` selects the currently active one.

## When prompt changes take effect

Prompt templates do not hot-rewrite the current thread. They primarily affect:

- new threads
- new sessions
- new system-prompt preview results

Already established threads are not rewritten retroactively.

If you changed a template pack and the current conversation did not change, do not assume the save failed. First check whether you are still inside an existing thread.

## What a Skill is

A Skill can be understood as a structured operating package for the model.

It usually contains:

- `SKILL.md`
- scripts
- example files
- resource directories

Once enabled, the model receives the Skill protocol and Skill entry information, and can use `skill_call` to read the full Skill content when needed.

## Skill layers

wunder currently groups skills into three layers:

- `builtin`
- `custom`
- `external`

This is really a governance boundary:

- built-in skills are read-only by default
- user-uploaded or admin-uploaded content goes into the custom directory
- external skills are closer to imported external capabilities

So "can this skill be modified?" does not have one universal answer. It depends on which layer the skill belongs to.

## Why `skill_call` matters

Many systems only expose the name of a skill but not the actual documentation, which forces the model to guess repeatedly.

wunder's built-in `skill_call` is explicit:

- it returns the complete `SKILL.md` by skill name
- it returns the skill directory structure
- it avoids overly aggressive clipping

This turns a Skill from "just a name in the prompt" into "an operating manual the model can actively read."

## What `{{SKILL_ROOT}}` is for

Resource paths inside a Skill should normally be written with:

- `{{SKILL_ROOT}}`

The reason is practical:

- the real filesystem path may differ across server, desktop, and cli
- hard-coded local paths break as soon as the skill moves

When `skill_call` returns the skill content, this placeholder is replaced with the absolute path visible to the model in the current runtime.

## How prompts, skills, and memory work together

The clean way to read them is:

- the system prompt defines the long-lived stable boundary
- a Skill defines the additional execution protocol for a class of work
- memory defines the long-term fact snapshot injected during thread initialization

All three matter, but they should not be mixed together conceptually.

## Common misunderstandings

### Misunderstanding 1: changing the prompt template changes the current thread immediately

It does not.

It primarily affects new threads.

### Misunderstanding 2: a Skill is just a documentation file

Not in wunder.

It is a runtime resource the model can explicitly load and read.

### Misunderstanding 3: memory rewrites the system prompt every round

It does not.

The current rule is one injection during initialization, then the thread prompt remains frozen.

## Further reading

- [Tool System](/docs/en/concepts/tools/)
- [Long-term Memory](/docs/en/concepts/memory/)
- [Configuration Reference](/docs/en/reference/config/)
- [Streaming Events Reference](/docs/en/reference/stream-events/)
