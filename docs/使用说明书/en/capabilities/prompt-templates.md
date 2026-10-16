---
title: Prompts and Personalization
summary: The built-in packs `default-zh` and `default-en`, the segments a pack is made of, switching and effect rules, and how to create a custom pack.
updated_at: 2026-10-16
---

A prompt template sets the agent's role and answer style. Two built-in packs ship with wunder — `default-zh` and `default-en` — and custom packs can be created in the interface.

## Prompt templates

A template sets four things:

- The role
- How to think and answer
- The working principles and limits
- How to use tools and skills

Switch to another template and the agent's style and emphasis change with it.

## Built-in template packs

| Pack | Language | Fits |
| --- | --- | --- |
| `default-zh` | Thinks and answers in Chinese | Chinese-language conversations; the default Chinese assistant style |
| `default-en` | Thinks and answers in English | English-language conversations; the default English assistant style |

Pick the pack that matches the language the user mostly works in. A template only sets a default tendency: even with `default-zh`, asking for English in the conversation is followed.

## What a pack is made of

Each pack is assembled from several segments:

| Segment | Purpose |
| --- | --- |
| Role | Defines who the agent is and what role it plays |
| Engineering principles | Defines how it works and thinks |
| Tool protocol | Defines how it uses tools |
| Skill protocol | Defines how it invokes skills |
| Memory | Defines how it uses long-term memory |
| Extra notes | Additional instructions |

## Switching packs

### Switching steps

1. Go to Settings at the bottom of the left sidebar → Agent
2. Find the prompt template option
3. Choose the pack you want
4. Save

### When it takes effect

- **New conversations**: ones created after the switch use the new template
- **Existing conversations**: unchanged, keeping the template from when they were created

A conversation fixes its prompt when it is created and then keeps it frozen, so behavior stays consistent and does not shift partway through. An existing conversation's prompt cannot be edited directly: start a new conversation to use the new template, or copy the content you want to keep into a new conversation and continue there.

## Creating a custom template

### Creation steps

1. In the template settings page, click New template pack
2. Pick a template to copy as the base; starting from a built-in one works well
3. Edit the content of each segment
4. Save, and it is ready to use

### Writing each segment

- **Role segment**: state the role, such as "coding assistant" or "data analysis expert"
- **Engineering principles**: set the answer style, concise or detailed, formal or casual
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

An upper layer overrides a lower one, and anything missing falls back to the lower layer. Each agent configures its template independently.

## Writing notes

- Start from a built-in template, then customize from there
- Create a separate pack for each scenario if you like
- State the key requirements clearly and avoid redundancy; longer is not better, and a long or self-contradictory template works against that
- Do not put contradictory requirements in the same template
- Review it occasionally and drop entries you no longer need
