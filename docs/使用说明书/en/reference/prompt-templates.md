---
title: Prompts and Personalization
summary: Customize the agent's role and answer style — pick a built-in template, or write your own.
updated_at: 2026-10-13
---

A prompt template sets the agent's role and answer style. Pick a built-in one, or write your own, and the agent works to that agreement.

## Prompt templates

A prompt template is the agent's handbook. It defines:

- The role
- How to think and answer
- The working principles and limits
- How to use tools and skills

Switch to another template and the agent's style and emphasis change with it.

## Built-in template packs

wunder ships two built-in packs.

### Chinese pack (default-zh)

- Thinks and answers in Chinese
- Fits Chinese-language conversations
- The default Chinese assistant style

### English pack (default-en)

- Thinks and answers in English
- Fits English-language conversations
- The default English assistant style

Pick the pack that matches the language the user mostly works in.

## What a pack is made of

Each pack is assembled from several segments that together form the agent's complete handbook:

| Segment | Purpose |
|------|------|
| Role | Defines who the agent is and what role it plays |
| Engineering principles | Defines how it works and thinks |
| Tool protocol | Defines how it uses tools |
| Skill protocol | Defines how it invokes skills |
| Memory | Defines how it uses long-term memory |
| Extra notes | Additional instructions |

## Switching packs

### Switching in the interface

1. Go to Settings at the bottom of the left sidebar → Agent
2. Find the prompt template option
3. Choose the pack you want
4. Save

### When it takes effect

- **New conversations**: ones created after the switch use the new template
- **Existing conversations**: unchanged, keeping the template from when they were created

## Creating a custom template

### How to create one

1. In the template settings page, click New template pack
2. Pick a template to copy as the base (starting from a built-in one is recommended)
3. Edit the content of each segment
4. Save, and it is ready to use

### Editing tips

- **Role segment**: state the role, such as "coding assistant" or "data analysis expert"
- **Engineering principles**: set the answer style (concise or detailed, formal or casual)
- **Tool protocol**: usually leave it alone
- **Extra notes**: put your own specific requirements here

### Example: a coding assistant template

The role segment might read:

```
You are a professional coding assistant, skilled in several programming languages and software engineering practices.
Your answers should:
- show clear, complete code examples
- explain in a concise, easy-to-follow way
- follow best practices
- pay attention to security
```

## How templates layer

The agent's final prompt is combined from several layers:

1. **System template**: the global base set by an administrator
2. **User template**: the pack the user picked
3. **Agent settings**: extra configuration for that particular agent

An upper layer overrides a lower one, and anything missing falls back to the lower layer.

## Common situations

### Why an existing conversation keeps its template

A conversation fixes its prompt when it is created and then keeps it frozen, so behavior stays consistent and does not shift partway through.

### Changing one conversation's prompt

An existing conversation's prompt cannot be edited directly. Start a new conversation to use the new template, or copy the content you want to keep into a new conversation and continue there.

### Different agents can use different templates

Each agent configures its template independently.

### Longer is not always better

A template should state the key requirements clearly and avoid redundancy and contradiction; a long or self-contradictory one runs counter to that.

### A Chinese template can still answer in English

The template only sets a default tendency; ask for English in the conversation and the agent will follow.

## Usage notes

- Start from a built-in template, then customize from there
- Create a separate pack for each scenario if you like
- State the key requirements clearly and keep it short
- Do not put contradictory requirements in the same template
- Review it occasionally and drop entries you no longer need

## Further reading

- [Feature and Capability Quick Reference](/docs/en/reference/api-index/) — where agent management lives
- [Prompts and Skills](/docs/en/concepts/prompt-and-skills/)
- [Long-term Memory](/docs/en/concepts/memory/)
