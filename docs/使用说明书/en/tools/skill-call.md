---
title: Skill Call
summary: The `skill_call` argument, how the returned body is assembled and `{{SKILL_ROOT}}` resolved, and the four immediate failures.
updated_at: 2026-10-16
---

The argument is the skill name; `skill_call` loads that skill's body into the current conversation so the agent can carry on by following its process.

```json
{
  "name": "openai-docs"
}
```

## Return value

- It formats the skill instructions so they are easy for the model to read, then puts them into context.
- It states the skill's root directory explicitly; a `{{SKILL_ROOT}}` placeholder in the instructions resolves to that root.
- It also lists the top-level files and folders under the skill root, so the agent knows what supporting material it can read next.

## Errors

An empty skill name, a skill that does not exist, an ambiguous name, or a skill file that cannot be read all fail immediately. These are ordinary errors, not asynchronous work that needs waiting.
