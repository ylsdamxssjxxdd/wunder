---
title: Core Concepts
summary: The eleven core concepts are not eleven feature bullets. They are the structural boundaries wunder establishes for long-running threads, concurrent users, and multiple entry surfaces.
read_when:
  - You are trying to understand wunder systematically for the first time
  - You are preparing for integration, operations, or tool development and need one shared perspective
  - You already know how to use wunder, but want to understand why it is designed this way
source_docs:
  - docs/总体设计.md
---

# Core Concepts

To understand wunder, do not begin by memorizing terms, endpoints, or page names. Start with the eleven core concepts, because they define not just what modules exist, but which stability boundaries the system must hold.

The older topic pages were not removed. They were moved under [Reference Overview](/docs/en/reference/) as runtime-model reference material. This page builds the main structural model and then points you to the eleven dedicated core pages.

![The eleven core layers of wunder: execution kernel, access and governance, and delivery assurance](/docs/assets/manual/core-overview-map.svg)

## First remember these four judgments

- wunder is not centered on one-off answers. It is centered on whether a thread can keep running stably over time.
- wunder does not bury capabilities inside prompts when it can instead formalize them as tools, events, and governance constraints.
- wunder is not built for only one chat entry. Server, desktop, cli, and channel adapters are meant to share one kernel.
- wunder does not add governance after feature work. It starts from concurrent multi-user access, long conversations, and high-risk toolchains by default.

## Why these eleven are core rather than ordinary features

| Dimension | What breaks if it is not treated as a core concept |
|------|------|
| Execution flow | the system collapses into one-shot Q&A, and thread semantics stop converging cleanly |
| Capability organization | tools, memory, swarms, and channels fragment, making them hard for both model and frontend to consume |
| Governance boundaries | multi-user access, permissions, scheduled tasks, and external integration contaminate each other later |
| Operations and replay | you can only see that the result was wrong, not why, where it stalled, or how to replay it |

## How the eleven core concepts are layered

| Layer | Core concept | The real problem it solves |
|------|------|------|
| Execution kernel | agent loop, tools, swarm, context compression, memory | keep threads running, make capabilities callable, support collaboration, control context, and reuse long-lived knowledge |
| Access and governance | channels, scheduled tasks, multi-user management | let multiple entry points, background jobs, and user boundaries share one system vocabulary |
| Delivery assurance | realtime behavior, stability, observability | make state visible to users, survivable for the system, and replayable for operators |

## The eleven core concepts at a glance

| Core concept | One-line meaning | Related reading |
|------------|----------|----------|
| Agent loop | keeps a thread moving through repeated cycles of think, act, observe, and continue | [Sessions and Rounds](/docs/en/concepts/sessions-and-rounds/), [Streaming Execution](/docs/en/concepts/streaming/) |
| Tools | make capability invocation reliable instead of limiting the model to text generation | [Tool System](/docs/en/concepts/tools/), [Prompts and Skills](/docs/en/concepts/prompt-and-skills/) |
| Swarm | lets a queen agent coordinate workers in parallel instead of stuffing everything into one thread | [Swarm Collaboration](/docs/en/concepts/swarm/) |
| Context compression | lets long conversations continue while preserving the useful information | [Boundary Handling](/docs/en/concepts/boundary-handling/), [Token Quota and Occupancy](/docs/en/concepts/quota-and-token-usage/) |
| Memory | keeps long-term facts usable without polluting the thread's core reasoning boundary | [Long-term Memory](/docs/en/concepts/memory/), [Workspaces and Containers](/docs/en/concepts/workspaces/) |
| Channels | lets server, desktop, cli, and third-party entry points share one kernel | [Architecture](/docs/en/concepts/architecture/), [Integration Overview](/docs/en/integration/) |
| Scheduled tasks | turns recurring execution and background governance into first-class system behavior | [Schedule Task Tool](/docs/en/tools/schedule-task/), [Operations Overview](/docs/en/ops/) |
| Multi-user management | makes organizations, tenants, permissions, and token-account governance first-class | [Server Deployment](/docs/en/start/server/), [Authentication and Security](/docs/en/ops/auth-and-security/) |
| Realtime | keeps frontends and external systems continuously aware of thread and task changes | [Streaming Events Reference](/docs/en/reference/stream-events/), [Chat WebSocket](/docs/en/integration/chat-ws/) |
| Stability | keeps the system running under long conversations, concurrency, and heavy tool usage | [Boundary Handling](/docs/en/concepts/boundary-handling/), [Troubleshooting](/docs/en/help/troubleshooting/) |
| Observability | lets the system explain what happened, why, and how to replay it | [Streaming Events Reference](/docs/en/reference/stream-events/), [Benchmarking and Observability](/docs/en/ops/benchmark-and-observability/) |

## Dedicated pages for the eleven core concepts

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/concepts/core-agent-loop/"><strong>Agent Loop</strong><span>Thread state machine, terminal-state convergence, and resume behavior.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-tools/"><strong>Tools</strong><span>Tool descriptions, structured arguments, and result constraints.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-swarm/"><strong>Swarm</strong><span>Queen, workers, and collaboration boundaries.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-context-compression/"><strong>Context Compression</strong><span>Long-thread compression, summary reinjection, and traceability.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-memory/"><strong>Memory</strong><span>Initialization-time injection and frozen prompt constraints.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-channels/"><strong>Channels</strong><span>Shared kernel across multiple entry surfaces.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-scheduled-tasks/"><strong>Scheduled Tasks</strong><span>Recurring execution, background governance, and execution history.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-multi-user-management/"><strong>Multi-user Management</strong><span>Tenants, permissions, token accounts, and governance panels.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-realtime/"><strong>Realtime</strong><span>Event streams, snapshot compensation, and reconnect recovery.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-stability/"><strong>Stability</strong><span>Error isolation, retries, recovery, and regression acceptance.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-observability/"><strong>Observability</strong><span>Fact streams, replay, profiling, and metric conventions.</span></a>
</div>

## Suggested reading order

- If you are working on integration, start with [Agent Loop](/docs/en/concepts/core-agent-loop/), [Channels](/docs/en/concepts/core-channels/), and [Realtime](/docs/en/concepts/core-realtime/).
- If you are working on tools or agent capability design, start with [Tools](/docs/en/concepts/core-tools/), [Context Compression](/docs/en/concepts/core-context-compression/), and [Memory](/docs/en/concepts/core-memory/).
- If you are working on admin surfaces or production governance, start with [Multi-user Management](/docs/en/concepts/core-multi-user-management/), [Stability](/docs/en/concepts/core-stability/), and [Observability](/docs/en/concepts/core-observability/).
- If you are working on collaboration and automation, start with [Swarm](/docs/en/concepts/core-swarm/) and [Scheduled Tasks](/docs/en/concepts/core-scheduled-tasks/).

## Overall principles

| Principle | Meaning |
|------|------|
| Everything returns to threads | sessions, tools, compression, swarms, and replay must all converge back to thread semantics |
| Everything returns to events | realtime sync, troubleshooting, replay, and monitoring should be built on events and state transitions |
| Everything returns to constraints | prompt freezing, one-time memory injection, consistent metrics, and separation between facts and profiles are hard constraints |
| Everything returns to performance | concurrent access, long threads, toolchain execution, and multi-frontend sync must be designed with speed and resource cost in mind |

## What to read next

- If you want the dedicated pages for each core concept, use the cards above.
- If you want the older runtime-model pages organized by topic, go to [Reference Overview](/docs/en/reference/).
- If you want to integrate the system directly, go to [Integration Overview](/docs/en/integration/).
- If you want the tool layer first, go to [Tools Overview](/docs/en/tools/).
- If you want constraints and operational failure handling, go to [Operations Overview](/docs/en/ops/) and [Troubleshooting](/docs/en/help/troubleshooting/).
