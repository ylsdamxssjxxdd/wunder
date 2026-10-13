---
title: Skill Call
summary: Loading a skill's instructions and its directory into the current conversation so the agent can work through that skill's process.
updated_at: 2026-10-13
---

A skill call loads a skill's instructions into the current conversation so the agent can carry on by following that skill's process.

## When to use it

- You want the agent to follow an existing skill instead of improvising.
- The agent needs to read the skill's instructions first before it can know what to do next.

## How to use it

Pass the skill name:

```json
{
  "name": "openai-docs"
}
```

## What it handles

- It formats the skill instructions so they are easy for the model to read, then puts them into context.
- It states the skill's root directory explicitly; a `{{SKILL_ROOT}}` placeholder in the instructions resolves to that root.
- It also lists the top-level files and folders under the skill root, so the agent knows what supporting material it can read next.

## When something goes wrong

An empty skill name, a skill that does not exist, an ambiguous name, or a skill file that cannot be read all fail immediately. These are ordinary errors, not asynchronous work that needs waiting.
