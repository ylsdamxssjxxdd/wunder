---
title: Core Concepts
summary: The four design principles, the four forms, the four cores, deeper pages by topic, and capabilities that are removed or restricted.
updated_at: 2026-10-16
---

wunder is an agent-scheduling platform: hand it a goal and the agent breaks the task down, calls tools, and delivers results. The feature list lives in [Tools](/docs/en/tools/) and the [Capability Quick Reference](/docs/en/capabilities/api-index/).

## Design Principles

| Principle | Meaning |
| --- | --- |
| **Everything revolves around the thread** | The thread (= session) is the unit of execution, not "one Q&A". A single agent can hold several independent task threads at once, and scheduling, locks, and realtime state are all isolated per thread |
| **Everything is a tool** | Everything the model can do is exposed through a unified tool interface; canonical names are mostly Chinese (`读取文件`, `执行命令`, `记忆管理`), and English aliases are normalized before routing |
| **One engine across forms** | There is only one core and one engine; the four forms differ only at the access layer |
| **Boundaries and governance are built in** | Prompt freezing, approval tiers, daily quotas, and bounded views with pagination are part of the skeleton, not left to the upper layers |

## Forms

| Form | Role |
| --- | --- |
| **Pupa** (desktop) | A local AI workbench, the default entry for individuals; a local desktop process that calls the engine in-process, with a local SQLite |
| **Beehive** (web) | A cloud AI workbench reached from a browser; no process of its own, using the Hull's storage |
| **Helm** (cli) | A hands-on local entry driven from a terminal and easy to script; a local process with a local SQLite |
| **Hull** (server) | The user- and agent-management platform: multi-tenancy, units, app build-and-release, and gateway access; a service process plus the engine, with the Bridge built in |

The Pupa and the Helm do not depend on the Hull and can run and ship on their own. The Electron / Tauri front-end shells are no longer maintained day to day and are no longer described as a capability.

## Cores

| Core | Meaning |
| --- | --- |
| **Fast** | Starts up quickly and responds quickly |
| **Stable** | The agent can work for long stretches without stalling or leaking memory, the program runs normally, and CPU usage stays low |
| **General** | Deployable on any operating-system platform, connectable to any mainstream model, able to complete all kinds of complex tasks |
| **Lightweight** | A small program body, light prompt and tool design, and low token consumption |

The per-item detail pages for the 9 capabilities — goal, compatibility, context compression, subagent, scheduled tasks, channels, memory, multithreading, and multi-tenancy — live under [Capabilities › Core Capabilities](/docs/en/capabilities/).

## Deeper Pages by Topic

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
| How a thread advances round by round | [Agent Loop](/docs/en/concepts/core-agent-loop/) |
| The four layers of capabilities | [Tools](/docs/en/concepts/core-tools/) |
| The process is visible and a disconnect can resume | [Realtime](/docs/en/concepts/core-realtime/) |
| Bounded views, budgets, and circuit breaking | [Stability](/docs/en/concepts/core-stability/) |
| Logs, metrics, and thread trajectories | [Observability](/docs/en/concepts/core-observability/) |

## Reading Paths

- **First time on the Pupa**: [Quickstart](/docs/en/start/quickstart/) → Agent Loop → Tools → Memory → Context Compression.
- **Administrator / governance**: Multi-user Management → Stability → Observability → Channels → [Configuration Reference](/docs/en/capabilities/config/).
- **Integration and development**: [Architecture](/docs/en/concepts/architecture/) → [Chat WebSocket](/docs/en/capabilities/chat-ws/) → Streaming Execution.

## Limits

- **"Several agents can form a swarm and collaborate"** — they cannot. The only collaborative form is the **subagent**: a temporary work unit the main agent creates and reclaims when the task ends. Swarm orchestration (swarm tools, hives, orchestration mode, team runs, swarm packs) has been removed from the engine and the scheduling surface; a few terminal strings and UI labels still carry the old wording, but that is leftover to clean up, not a capability.
- **"Every listed tool is usable"** — it is not. The default usable set is a fixed tier, and `网页搜索` and `ptc` are currently **temporarily withdrawn** (code kept, invisible to model and user), not deleted.
- **"Editing the prompt affects the current conversation at once"** — it does not. Once a thread first fixes its system prompt, it is frozen.
- **"What the UI shows is the back-end truth"** — the UI consumes a bounded view (pagination plus a byte budget), not the full history. When the content looks wrong, trust the thread ledger.
- **"Stopping maintenance of the front-end shell stops the Pupa back end"** — they are two different things. The Pupa runs the local native UI with in-process native calls by default, and its execution, tools, and storage semantics still reuse the engine.
