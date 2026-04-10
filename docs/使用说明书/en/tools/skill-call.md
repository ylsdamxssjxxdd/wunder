---
title: Skill Call
summary: How `skill_call` returns the skill body, root directory, and file tree to the model.
read_when:
  - You need to load a `SKILL.md` file and its directory context
source_docs:
  - src/services/tools.rs
updated_at: 2026-04-10
---

# Skill Call

`skill_call` does not execute a skill. It **loads the skill content into the current context** so the model can continue by following that skill's workflow.

## Minimum arguments

```json
{
  "name": "openai-docs"
}
```

## Success result

```json
{
  "ok": true,
  "action": "skill_call",
  "state": "completed",
  "summary": "Loaded skill openai-docs.",
  "data": {
    "name": "openai-docs",
    "description": "Use official OpenAI docs...",
    "path": "C:/.../SKILL.md",
    "root": "C:/.../openai-docs",
    "content": "# SKILL ... {{SKILL_ROOT}} ...",
    "tree": [
      "SKILL.md",
      "references/",
      "scripts/"
    ]
  }
}
```

## Key changes

- `content` is rendered in a form that is easier for the model to read
- `root` explicitly returns the skill root directory
- `{{SKILL_ROOT}}` can be used as a placeholder inside the content
- `tree` tells the model what additional files exist under the skill directory

## Common failures

- the skill name is empty
- the skill does not exist
- the skill name is ambiguous
- the skill file could not be read

These failures are usually returned as ordinary direct errors. This is not a complex asynchronous tool.
