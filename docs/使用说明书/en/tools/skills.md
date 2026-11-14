---
title: Skills
summary: What a skill is (a directory, `SKILL.md`, an entry script), its two uses, skill names and the `owner@skill` form, mounting and ability selection, and the 60-second subprocess boundary.
updated_at: 2026-11-14
---

# Skills

A skill packages knowledge and scripts: one directory, a `SKILL.md` with frontmatter (name, description), and an entry script looked up in a fixed order. It works both as a callable tool and as reference material you only read.

## Two uses

| Use | Path | Good for |
| --- | --- | --- |
| Call it as a tool | The skill name is itself a callable tool name | You already know which skill to use |
| Read its body | `技能调用` (alias `skill_call`) returns name, description, path, root, and body | You want the workflow or only its knowledge before deciding |

`技能调用` replaces skill-root placeholders in the body with real paths, so the model can read files and run scripts with what it gets back. It does not run the entry script — execution is a separate call.

## Names and ownership

- A bare name (`文稿校对`) is a skill you have.
- A skill shared by someone else is written `owner@skill`.
- On a name collision wunder does not guess: it reports the ambiguity and lists the candidates.

## Entry script

Looked up in order `run.py` → `skill.py` → `main.py`, first hit wins. Importing a skill package (archives and collisions included) is the platform's job, so a script never has to unpack itself.

The global skill directory comes from configuration, and the script executor path can be overridden.

## Mounting and selection

- Built-in skills ship with the package and are marked apart from your own; user skills route by name like MCP servers and knowledge bases.
- "Selecting an ability" only puts a tool or skill name into that agent's selection set; the skill layer still does the executing.
- Model availability filters apply before anything reaches the context, and thresholds differ per capability.

## Execution boundary

A skill runs as a **subprocess executing a script**, with a 60-second timeout, inheriting the platform's path and permission boundary. Do not put privileged operations in a skill script. If it will not run, first check that the entry script exists, then whether `技能调用` can read the body at all — if it cannot, the directory or the frontmatter is usually the problem.

For MCP see [MCP](/docs/en/tools/mcp/); the overall name-routing rules are on [Tools Overview](/docs/en/tools/).
