---
title: Core Concepts
summary: Why wunder behaves the way it does—10 core design constraints, along with the layers they form, the entries into them, and the reading paths.
updated_at: 2026-10-13
---

wunder is an agent-scheduling platform: hand it a goal and it breaks the task down, calls tools, and delivers results. The 10 core concepts here explain **why the system behaves the way it does**, not a list of what it can do. For the feature list, see [Tools](/docs/en/tools/) and the [API Index](/docs/en/reference/api-index/).

![The ten core layers of wunder: the execution engine, access and governance, and delivery assurance](/docs/assets/manual/core-overview-map.svg)

## Four design principles

| Principle | Meaning |
| --- | --- |
| **Everything revolves around the thread** | The thread (= session) is the unit of execution, not "one Q&A". A single agent can hold several independent task threads at once, and scheduling, locks, and realtime state are all isolated per thread |
| **Everything is a tool** | Everything the model can do is exposed through a unified tool interface; canonical names are mostly Chinese (`读取文件`, `执行命令`, `记忆管理`), and English aliases are normalized before routing |
| **One engine across forms** | There is only one core and one engine; the four forms differ only at the access layer |
| **Boundaries and governance are built in** | Prompt freezing, approval tiers, daily quotas, and bounded views with pagination are part of the skeleton, not left to the upper layers |

## The four forms

| Form | Role |
| --- | --- |
| **Honeycomb** (desktop) | A local AI workbench, the default entry for individuals; a local desktop process that calls the engine in-process, with a local SQLite |
| **Beehive** (web) | A cloud AI workbench reached from a browser; no process of its own, using the Hull's storage |
| **Helm** (cli) | A hands-on local entry driven from a terminal and easy to script; a local process with a local SQLite |
| **Hull** (server) | The user- and agent-management platform: multi-tenancy, units, app build-and-release, and gateway access; a service process plus the engine, with the Bridge built in |

The Honeycomb and the Helm do not depend on the Hull and can run and ship on their own. The Electron / Tauri front-end shells are no longer maintained day to day and are no longer described as a capability.

## The 10 core concepts

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/concepts/core-agent-loop/"><strong>Agent Loop</strong><span>How a thread advances round by round to a terminal state, and what brakes exist.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-tools/"><strong>Tools</strong><span>The agent does more than talk: tools are the unified interface to execution.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-context-compression/"><strong>Context Compression</strong><span>Why a long conversation does not break off when it exceeds the limit.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-memory/"><strong>Memory</strong><span>How long-term material is injected once and safely joins execution.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-channels/"><strong>Channels</strong><span>How third-party platform entries share one thread runtime.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-scheduled-tasks/"><strong>Scheduled Tasks</strong><span>How due messages queue without fighting human messages for the lock.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-multi-user-management/"><strong>Multi-user Management</strong><span>Organizations, permissions, quotas, and the governance surface.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-realtime/"><strong>Realtime</strong><span>The process is visible, a disconnect can resume, and what you see is what was stored.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-stability/"><strong>Stability</strong><span>Errors do not spread, the system can heal itself, and retries are bounded.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-observability/"><strong>Observability</strong><span>Thread trajectories and ledgers can explain what happened.</span></a>
</div>

The layers, from the inside out:

| Layer | Cores | What they solve |
| --- | --- | --- |
| Execution engine | loop, tools, context compression, memory | A thread can keep running, invoke capabilities, control its length, and use long-term material |
| Access and governance | channels, scheduled tasks, multi-user management | Different entries and background jobs share one set of rules |
| Delivery assurance | realtime, stability, observability | The process is visible, the pressure is bearable, and problems can be traced back |

## Going deeper by topic

One layer down, by topic (all under [Reference › In Depth](/docs/en/reference/)):

| Topic | Page |
| --- | --- |
| Which stages a message passes through | [Architecture](/docs/en/concepts/architecture/) |
| How sessions, threads, and rounds are numbered | [Sessions and Rounds](/docs/en/concepts/sessions-and-rounds/) |
| Realtime push and resuming after a disconnect | [Streaming Execution](/docs/en/concepts/streaming/) |
| Workspaces and file safety | [Workspaces and Working Directory](/docs/en/concepts/workspaces/) |
| Status indicators | [Runtime State](/docs/en/concepts/presence-and-runtime/) |
| Tool sources and governance | [Tool System](/docs/en/concepts/tools/) |
| The four layers of prompts, skills, and capabilities | [Prompts and Skills](/docs/en/concepts/prompt-and-skills/) |
| Memory shapes and troubleshooting | [Long-term Memory](/docs/en/concepts/memory/) |
| Tokens and quotas | [Tokens and Quotas](/docs/en/concepts/quota-and-token-usage/) |
| Persistent goal mode | [Goal Mode](/docs/en/concepts/goal-mode/) |
| Animated companions | [Companion System](/docs/en/concepts/companion/) |
| Anomaly and truncation behavior | [Boundary Handling](/docs/en/concepts/boundary-handling/) |

## Recommended reading by role

- **First time on the Honeycomb**: [Quickstart](/docs/en/start/quickstart/) → Agent Loop → Tools → Memory → Context Compression.
- **Administrator / governance**: Multi-user Management → Stability → Observability → Channels → [Configuration Reference](/docs/en/reference/config/).
- **Integration and development**: [Architecture](/docs/en/concepts/architecture/) → [Hull API](/docs/en/integration/wunder-api/) → [Chat WebSocket](/docs/en/integration/chat-ws/) → Streaming Execution.

## Common misconceptions

- **"Several agents can form a swarm and collaborate"** — they cannot. The only collaborative form is the **subagent**: a temporary work unit the main agent creates and reclaims when the task ends. Swarm orchestration (swarm tools, hives, orchestration mode, team runs, swarm packs) has been removed from the engine and the scheduling surface; a few terminal strings and UI labels still carry the old wording, but that is leftover to clean up, not a capability.
- **"Every listed tool is usable"** — it is not. The default usable set is a fixed tier, and `网页搜索` and `ptc` are currently **temporarily withdrawn** (code kept, invisible to model and user), not deleted.
- **"Editing the prompt affects the current conversation at once"** — it does not. Once a thread first fixes its system prompt, it is frozen.
- **"What the UI shows is the back-end truth"** — the UI consumes a bounded view (pagination plus a byte budget), not the full history. When the content looks wrong, trust the thread ledger.
- **"Stopping maintenance of the front-end shell stops the Honeycomb back end"** — they are two different things. The Honeycomb runs the local native UI with in-process native calls by default, and its execution, tools, and storage semantics still reuse the engine.

## Further reading

- [Architecture](/docs/en/concepts/architecture/) — stages and links
- [Sessions and Rounds](/docs/en/concepts/sessions-and-rounds/) — counting conventions
- [Glossary](/docs/en/help/glossary/) — consistent wording
