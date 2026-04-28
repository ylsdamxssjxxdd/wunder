---
title: Prompt Templates
summary: Wunder splits system prompts into template packs and segment files. Administrators manage system template packs. The user side currently provides two built-in template packs (Chinese and English) by default and supports switching or copying them into custom packs.
read_when:
  - You want to modify system prompt template packs
  - You want to understand the relationship between `default-zh`, `default-en`, and custom packs on the user side
  - You are troubleshooting why switching template packs takes effect for new threads but not existing ones
source_docs:
  - docs/API文档.md
  - docs/设计文档/01-系统总体设计.md
  - src/api/admin_prompt_templates.rs
  - src/api/user_prompt_templates.rs
---

# Prompt Templates

This page only covers how template packs are organized, how fallback works, and when changes take effect. It does not re-explain "why prompts are needed."

## Current Segments

System prompt templates are organized into these segments:

- `role`
- `engineering`
- `tools_protocol`
- `skills_protocol`
- `memory`
- `extra`

You manage these segment files, not one monolithic hardcoded prompt.

## Admin-Side Template Packs

Endpoints:

- `GET /wunder/admin/prompt_templates`
- `POST /wunder/admin/prompt_templates/active`
- `GET/PUT /wunder/admin/prompt_templates/file`
- `POST /wunder/admin/prompt_templates/packs`
- `DELETE /wunder/admin/prompt_templates/packs/{pack_id}`

Constraints:

- The `default` pack is read-only
- Non-`default` packs are stored in `./config/data/prompt_templates`
- After an admin switches the active pack, new system template reads will prioritize the current active pack, falling back to the system `default` for any missing segments

## User-Side Template Packs

Endpoints:

- `GET /wunder/prompt_templates`
- `POST /wunder/prompt_templates/active`
- `GET/PUT /wunder/prompt_templates/file`
- `POST /wunder/prompt_templates/packs`
- `DELETE /wunder/prompt_templates/packs/{pack_id}`

The user side currently provides two read-only built-in template packs:

- `default-zh`: reads Chinese system templates
- `default-en`: reads English system templates

Notes:

- On first use, or when a historical setting still uses the compatibility alias `default`, the system automatically resolves to `default-zh` or `default-en` based on the current system language
- The user interface displays `default-zh` and `default-en` by default, and no longer shows `default` as a primary option
- After a user manually switches to `default-zh` or `default-en`, the selected language template is fixed and will no longer drift with system language changes
- Both built-in packs are read-only; they essentially mirror the currently active admin system template pack but with different language locks
- User-created custom packs only affect new threads for that user going forward

## User-Side Fallback Chain

You can understand the resolution order as follows:

1. Look up the user's currently selected pack
2. If it is a custom pack, read the language-specific segments from the custom pack first
3. When the custom pack is missing segments, fall back to the current admin active system template pack
4. When the system template pack is also missing segments, fall back to the system `default`

For built-in packs:

- `default-zh` only reads from the Chinese template chain
- `default-en` only reads from the English template chain

So even if a user changes the interface language, as long as they have explicitly selected the Chinese or English pack, the runtime system prompt will continue using the language version bound to that pack.

## Creating a Custom Pack

When a user creates a new template pack, it typically "copies the currently selected template pack":

- If the current selection is `default-zh` or `default-en`, an editable copy is created from the current system template content
- If the current selection is a custom pack, the copy is derived from that custom pack

This allows users to select a Chinese or English built-in pack first, then quickly derive their own styled pack.

## When Changes Take Effect

The rules for when template updates take effect are straightforward:

- New threads will build their system prompt using the latest templates
- Existing threads whose system prompt has already been initialized and frozen will not be rewritten

This is consistent with the design of freezing system prompts at thread initialization time, and is not an anomaly.

## Implementation Recommendations

- To modify built-in default content, first copy from `default-zh` or `default-en` into a custom pack, then edit that
- To lock in a Chinese or English style, simply switch to the corresponding built-in pack
- To change the default style for all users on the user side, have the admin switch the system active template pack first

## Further Reading

- [Prompts & Skills](/docs/en/concepts/prompt-and-skills/)
- [Chat Sessions](/docs/en/integration/chat-sessions/)
- [Configuration Reference](/docs/en/reference/config/)
