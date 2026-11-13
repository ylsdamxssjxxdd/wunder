---
title: Extensions and Dynamic Tools
summary: The four layers of tools / skills / abilities / user tools and their dispatch order, plus how MCP, skills, and knowledge bases register, route, and land dynamically.
updated_at: 2026-11-13
---

# Extensions and Dynamic Tools

Some tool names the model sees come from the built-in table; others are registered dynamically from configuration at runtime. Dispatch works across four layers: tools, skills, abilities, and user tools.

## The four layers

| Layer | What it is | Who uses it |
| --- | --- | --- |
| **Tool** | An executor: it has an argument schema and a dispatch branch, and can produce side effects or read data | Called by the model, executed by the runtime |
| **Skill** | Knowledge + scripts: a directory containing `SKILL.md` (with frontmatter) and an entry script | Can be called as a tool, or read as reference material |
| **Ability** | An entry in the administrator's selection layer, not an executor: folds tools and skills into selectable, grantable items | Checked, granted, and shared in the Admin / Cloud |
| **User tool** | A user-supplied extension: MCP / skill / knowledge base, routed by alias | Configured on the user side, matched by alias at runtime |

"Selecting an ability" means putting a tool name or skill name into that agent's selection set; the actual execution is still done by the tool layer or skill layer.

## Dispatch order

Tool dispatch has a fixed hit order:

1. Normalize the English alias to the Chinese canonical name.
2. **User-tool alias table hit** → route directly to your user MCP, user skill, or user knowledge base. Your own skills can be called by bare name; another owner's skill is written as `owner@skill-name`.
3. **Skill-name hit** → execute the skill entry script directly.
4. **Name contains `@`** → treat it as MCP.
5. **Knowledge-base-name hit** → run retrieval against that base.
6. Everything else goes to built-in tools.

Note that step 2 **takes precedence over built-in tools**: when a user tool and a built-in tool share a name, the user tool changes the route. This is expected behavior.

## MCP

### The `server@tool` client

After an external MCP server is connected, every allowed tool is registered as `server-name@tool-name`.

- **Packed mode**: when a server is marked as packed, it folds into **one** tool with only three actions — list available tools, fetch one tool's description, and call one tool. This keeps dozens of work details out of the context at once; the model looks them up on demand.
- **Only one transport**: streamable-http. Old transport types are rejected when listing tools or calling; stdio is not supported on the wunder side.
- The tool list has a short cache: 30 seconds, at most 128 entries.
- Arguments must be objects; non-objects are treated as no arguments.
- **Return shape**: a direct call returns the MCP server's own result structure; a call routed through a user-tool alias is wrapped in the shared envelope.

### The `/wunder/mcp` server

The Server / Desktop mounts an MCP service at `/wunder/mcp` exposing two tools (for external access, see [MCP endpoint access](/docs/en/capabilities/mcp-endpoint/)):

- `excute`: runs one agent task and returns the final reply (the tool name really is spelled this way; treat it as authoritative when integrating)
- `doc2md`: parses a document and returns Markdown

When the endpoint runs a task, it computes the tool set for the agent on the fly and explicitly removes itself, so an endpoint-driven agent cannot call the endpoint back.

### Example service

There is also a standalone MCP service example registering database query, database export, knowledge-base query, and PPT read / write / delete / refine / template-read tools. To connect it, configure a streamable-http endpoint by the rules above.

## Skills

- A skill is a directory: `SKILL.md` with frontmatter, plus an entry script probed in a fixed order (`run.py` → `skill.py` → `main.py`).
- **The skill name itself is a callable tool name**; it does not need to go through "skill call".
- `技能调用` (alias `skill_call`) is responsible for "reading the body": it returns the skill name, description, path, root directory, and body, and replaces the skill-root placeholder in the body with the real path. The name may be `owner@skill-name` or a bare skill name; duplicates produce an ambiguity error listing the candidates.
- Packaged import (including compressed archives and duplicate-name handling) is handled by the platform.
- The global skills directory comes from configuration, and the script executor path can be overridden.

## Knowledge bases

- **Registered dynamically by base name**: every enabled base name becomes a tool name directly; when it collides with a skill name, the skill wins.
- Arguments: a query string or an array of keywords (keywords take precedence; multiple keywords are retrieved in groups and deduplicated), plus an optional limit.
- Three base types: literal retrieval, built-in vector retrieval, and external RAGFlow. When vector retrieval fails or no embedding profile is available, it **falls back to literal retrieval**, and the result shows whether fallback mode is active.
- Zero hits suggest narrowing the query; document items in the result are compacted to save context.

## Errors and handling

| Symptom | Check first |
| --- | --- |
| MCP configured but no tools | Whether the server is enabled, whether the allowlist blocks it, whether the transport type is a legacy value |
| Told the legacy transport type is no longer supported | Switch to streamable-http |
| Told the built-in tool is unknown | The name is neither a canonical name / alias, nor contains `@`, nor a skill name, nor a base name |
| Skill call fails | Whether the entry script is missing or exits non-zero; first check whether skill call can read the body |
| Knowledge-base name cannot be called | Whether the base is enabled, whether its root directory is empty, or whether a same-name skill occupies the name |
| A user tool "steals" a built-in name | Alias routing takes precedence over built-in dispatch; this is expected |
| A tool disappears entirely | It may be temporarily hidden or desktop-hidden; see [Tools](/docs/en/tools/index/) |

## Limits

- Dynamic names share one table with built-in names: avoid built-in canonical names and aliases when naming skills / knowledge bases, or the alias will rewrite the meaning.
- MCP tool listing and calls are both constrained by the allowlist, and packed folding still validates each time.
- Skill execution **runs a script in a subprocess** with a 60-second timeout; it inherits the platform's path and permission boundaries, so do not put privileged operations in skill scripts.
- Before an extension enters the model context, it still passes through the ability selection set and model-availability filters (vision, multimodal, and desktop thresholds differ; see [Tools](/docs/en/tools/index/)).

## Related

- [Tools](/docs/en/tools/index/) — the built-in tool matrix and shared result shell
- [MCP endpoint access](/docs/en/capabilities/mcp-endpoint/) — the external `/wunder/mcp` surface
- [Skills and prompts](/docs/en/concepts/prompt-and-skills/) — how skills bind to prompts
