---
title: wunder
summary: An agent system that gets things done. You set the goal, it breaks it down, calls tools, and delivers results.
---

# wunder

wunder is an **agent system that executes tasks**. You tell it your goal, and it automatically breaks it down, calls tools, coordinates in parallel, and delivers results.

It's not a chatbot — it's an **AI workbench that gets things done**.

| Mode | For Who | One-Line Summary |
|------|---------|------------------|
| **Desktop** | Individual users | Download and start using |
| **Server** | Teams / Organizations | Multi-user, unified management |
| **CLI** | Developers | Terminal-driven, scriptable |

## System Structure

![wunder System Structure](/docs/assets/manual/08-hierarchy-structure.svg)

Remember this one line:

```
wunder
  └─ User (your space)
      └─ Swarm (collaboration group)
          └─ Agent (the role that works)
              └─ Thread (a continuous conversation)
```

**You send a message → The swarm assigns it to the right agent → The agent keeps working in its thread.**

## Quick Start

→ [Quick Start Guide](/docs/en/start/quickstart/)

## Documentation

- [Desktop Guide](/docs/en/start/desktop/) — Download and go
- [Server Deployment](/docs/en/start/server/) — For teams and organizations
- [CLI Usage](/docs/en/start/cli/) — Terminal and automation
- [User Interface](/docs/en/surfaces/frontend/) — Chat, files, agents, tools
- [Help Center](/docs/en/help/) — FAQ, troubleshooting, glossary

## Key Concepts

- **Agent**: An AI role that executes tasks, with its own model, tools, and prompt configuration
- **Thread**: A continuous conversation within an agent
- **Swarm**: A group of agents collaborating on a task
- **Workspace**: Your persistent file space

→ [Glossary](/docs/en/help/glossary/)
