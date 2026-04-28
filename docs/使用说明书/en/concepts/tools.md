---
title: Tool System
summary: wunder is not built around "the model answers." It is built around "the model selects and orchestrates tools to finish work."
read_when:
  - You need to understand why wunder is more than chat
  - You need to understand tool sources, boundaries, and orchestration
source_docs:
  - docs/设计文档/01-系统总体设计.md
  - docs/API文档.md
  - docs/工具调用.md
---

# Tool System

The core of wunder is not "the model answers." The core is "the model orchestrates tools to complete tasks."

## Focus of this page

- where tools in wunder actually come from
- why tools from different sources can still be orchestrated together
- why tool visibility can change inside the same session

## Key conclusions

- For the model, capabilities should be unified as tools whenever possible instead of being scattered across implicit mechanisms.
- For developers, many system capabilities also need to become tools before they can be orchestrated, governed, and observed.
- Tool sources may differ, but the orchestration layer, display layer, and permission layer should stay as unified as possible.

## Where tools come from

At minimum, the current system includes:

- built-in tools
- MCP tools
- Skills
- knowledge-base tools
- user tools

## What built-in tools cover first

Built-in tools carry the most basic and most stable execution surface.

Common examples include:

- files and workspace: reading files, writing files, applying patches, searching content, listing files
- runtime operations: command execution, `ptc`, and sleep or wait
- web and desktop: `web_fetch`, browser automation, and desktop control
- sessions and collaboration: thread control, subagent control, agent swarm, and memory management

If you want to choose a specific tool right now, go directly to [Tools Overview](/docs/en/tools/).

## What MCP is for

MCP brings capabilities from outside wunder into the same orchestration space.

It does not replace built-in tools. It extends them by bringing in external services such as:

- databases
- external services
- third-party system bridges

## What Skills are for

A Skill is closer to a packaged method than to a single action.

It usually bundles:

- a method
- a sequence of steps
- a set of constraints
- best practices for a certain class of task

## What knowledge bases and user tools solve

- Knowledge bases provide controlled context sources, especially for rules, long documents, and domain material.
- User tools let different users extend the system for their own workflows while still staying inside the system's governance boundary.

## Why the tool system must be unified

If different tool sources all behave independently, several problems follow:

- the model cannot tell which class of capability to prefer
- the frontend cannot present and search tools consistently
- permissions, audit, and governance become fragmented
- troubleshooting becomes much harder because the failing layer is unclear

wunder's approach is: sources may differ, but orchestration and presentation should be unified whenever possible.

## Common misunderstandings

- `web_fetch`, browser automation, and desktop control are all tools, but they do not share the same capability boundary.
- A Skill is not just another name for a tool. It is closer to a model-readable operating manual.
- Tool visibility is not determined by code alone. It also depends on runtime form, configuration, and session context.

## Further reading

- [Tools Overview](/docs/en/tools/)
- [Web Fetch](/docs/en/tools/web-fetch/)
- [MCP Endpoint](/docs/en/integration/mcp-endpoint/)
- [A2A Interface](/docs/en/integration/a2a/)
- [Architecture](/docs/en/concepts/architecture/)
