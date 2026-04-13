---
title: Tools
summary: The Tools core explains why Wunder must unify all capabilities as tools, and why tools must simultaneously serve model invocation, execution governance, and frontend display.
read_when:
  - You are designing built-in tools, MCP, or Skills
  - You want to understand why tool returns must be concise and clear
source_docs:
  - docs/总体设计.md
---

# Tools

Wunder's focus is not "making models respond more like humans," but "enabling models to stably invoke the correct capabilities and consume the returned results."

![Tools layer diagram: tool specification, execution layer, model input results, display projection—four layers of relationship](/docs/assets/manual/core-tools.svg)

## Key Takeaways

- Tools are not an add-on capability list, but the unified interface through which the system exposes capabilities to models.
- A tool that's easy for humans to read but hard for models to invoke is a bad tool in Wunder.
- Tool design must simultaneously consider invocation accuracy, context cost, permission governance, and frontend workflow display.

## Why It Must Be Listed as a Core

Without "tools" as a core, Wunder would experience two types of degradation:

- Capabilities scattered across prompts, implicit logic, and temporary interfaces—models won't know how to stably invoke them.
- Frontend can only see "the model saying what it did," but cannot see what was actually executed.

Wunder unifying capabilities into tools essentially unifies three things: model mental model, execution protocol, and display protocol.

## What This Core Truly Protects

- Protects model invocation quality: Parameters and descriptions must minimize model guessing.
- Protects execution governance: Budgets, permissions, approvals, and failure feedback must enter a unified pipeline.
- Protects frontend visibility: Tool workflows need to be stably displayed, tracked, and replayed.

## Key Constraints

| Constraint | Purpose |
|------|------|
| Descriptions must be clear | Reduces model misuse and hallucinatory invocations |
| Parameters must be structured | Makes model invocation more like "filling a form" rather than "freestyling" |
| Returns must be concise and clear | Prevents observation noise from polluting context in return |
| Facts and display must be separated | UI cards are projections, not the tool's actual execution results |

## Design Focus

### Focus One: Wunder Doesn't Just Make Models "Talk," It Makes Models "Do"

Files, commands, browsers, desktops, user worlds, channels, memory management—these are all fundamentally execution capabilities and shouldn't each have their own protocols.

### Focus Two: Tool Results Should Serve Scheduling First, Then Display

A tool returning tens of thousands of characters of raw logs may seem information-rich to users, but is usually a disaster for models. Truly good tool returns prioritize delivering:

- The most critical facts at the moment
- Signals needed to proceed to the next step
- Actionable hints when failure occurs

### Focus Three: A Unified Tool System Is More Important Than "More Tools"

Built-in tools, MCP, Skills, knowledge bases, and user tools come from different sources, but ultimately must all enter a unified scheduling perspective. Otherwise, permissions, auditing, retrieval, and workflow display will all scatter.

## Common Misconceptions

- Skills are not a parallel world alongside tools—they are essentially solidified process capabilities.
- More tools isn't necessarily better; vague descriptions and overlapping boundaries directly lower model invocation quality.
- "Can return a lot of content" is not a virtue; being consumable by models at low cost is.

## Boundaries with Other Cores

- Difference from [Agent Loop](/docs/en/concepts/core-agent-loop/): Agent loop defines the main flow, tools define the invokable capabilities within the main flow.
- Difference from [Context Compression](/docs/en/concepts/core-context-compression/): Compression solves "what to do when results are too large," tools solve "how to expose capabilities to models."
- Difference from [Observability](/docs/en/concepts/core-observability/): Observability handles replaying tool trajectories, tools produce the trajectories.

## Further Reading

- [Core Overview](/docs/en/concepts/)
- [Tool System](/docs/en/concepts/tools/)
- [Prompts and Skills](/docs/en/concepts/prompt-and-skills/)
- [Tools Overview](/docs/en/tools/)