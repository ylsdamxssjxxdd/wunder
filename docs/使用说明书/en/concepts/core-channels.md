---
title: Core Channels
summary: Core Channels explains why Wunder allows multiple entry points, but absolutely forbids these entries from growing into multiple independent runtimes.
read_when:
  - You are integrating multiple entry points
  - You want to determine whether a capability belongs in the core or the integration layer
source_docs:
  - docs/总体设计.md
---

# Core Channels

Wunder has multiple runtime forms and access methods, but these entry points should not grow into mutually fragmented systems. Core Channels focuses on the balance between "how entry points diversify" and "how the core remains singular."

![Channels diagram: HTTP, WebSocket, Desktop, CLI, third-party channels all accessing a unified runtime core](/docs/assets/manual/core-channels.svg)

## Bottom Line

- Channels are just entry layer differences, not business core differences.
- New entries should prioritize reusing existing capabilities like threads, tools, events, and governance, rather than duplicating logic.
- The more channels you build, the more you need to converge, not create more exceptions.

## Why It Must Be a Core Concept

Without Channels as a core, the system easily evolves like this:

- Chat WebSocket has its own state machine.
- `/wunder` SSE has its own state machine.
- Desktop/CLI each adds another layer of special cases.
- Third-party channels, after protocol translation, casually rewrite business logic.

The end result is multiple entry points each doing their own thing, with thread semantics, tool semantics, and event semantics no longer unified.

## What This Core Truly Protects

- Protects core singularity: All entries share the same threads, tools, events, and governance capabilities.
- Protects protocol boundaries: The integration layer handles protocol adaptation, not redefining business reality.
- Protects evolution speed: New entries can extend without rebuilding an entire system.

## Key Constraints

| Constraint | Purpose |
|------|------|
| Multiple entries, one core | Avoid one system growing into multiple semi-independent products |
| Integration layer only does protocol adaptation | Prevent entry layer from tampering with thread, tool, and event semantics |
| New channels prioritize reusing existing capabilities | Control maintenance cost and behavioral drift |
| Local forms also preserve core semantics | Desktop and CLI cannot become exceptional systems |

## Design Priorities

### Priority 1: Channel diversification cannot break thread semantics

Regardless of whether messages come from web, desktop, CLI, or third-party platforms, they should ultimately fall back to the same thread and event system.

### Priority 2: Entry differences are "surface," not "skeleton"

Different channels can have different authentication, protocols, message formats, and connection methods, but these should all stop at the entry surface.

### Priority 3: More entries means unified semantics matter more

Channels are not about showing off by connecting more, but about making the same capabilities available across different entries. The real challenge is expanding entries without expanding chaos.

## Common Misconceptions

- Desktop, CLI, and Server are runtime form differences, not three independent product cores.
- Third-party channel integration is protocol adaptation, not re-implementing an orchestration system.
- More channels means more need for convergence, not more need for exceptional branches.

## Boundaries with Other Cores

- Difference from [Realtime](/docs/en/concepts/core-realtime/): Channels discuss "where it comes in from," realtime discusses "how state continuously goes out."
- Difference from [Multi-user Management](/docs/en/concepts/core-multi-user-management/): Channels discuss entry unification, multi-user management discusses governance unification.
- Difference from [Stability](/docs/en/concepts/core-stability/): Channels are responsible for not fragmenting, stability is responsible for not crashing.

## Further Reading

- [Core Overview](/docs/en/concepts/)
- [System Architecture](/docs/en/concepts/architecture/)
- [Integration Overview](/docs/en/integration/)
- [Chat WebSocket](/docs/en/integration/chat-ws/)