---
title: Tool System
summary: The full path a tool takes from registration and selection through dispatch to its result landing, along with parallelism, timeouts, budgets, and clipping conventions.
updated_at: 2026-10-13
---

Layered concepts, the default set, and naming conventions live in [Tools (core concepts)](/docs/en/concepts/core-tools/), and per-tool usage is in the [Tools overview](/docs/en/tools/).

## The path of one tool call

```
Model output
  → Parse and clean the tool call (repair parameters, restore values that were stringified)
  → Orchestration: policy and approval, hooks, timeout, budget, whether it can run in parallel
  → Dispatch: alias → skill → MCP → knowledge base → built-in
  → A unified result structure
  → Clipping before it reaches the model
  → Session record persistence + event push (the display channel is separate)
```

Two points are easy to get wrong:

- The four stages "pre → execute → post → result" only describe the model; **hooks are off by default** and are not the main execution chain.
- A business failure is not raised as an error in the dispatch layer but returned as a normal path. "The tool returned but the business did not succeed" is normal, and the model reads the reason for the failure.

## Why a tool is "invisible"

| Where it is stuck | Criterion |
| --- | --- |
| Not registered | The name is not in the canonical-name / alias table, and no matching MCP, skill, or knowledge base exists |
| Temporarily withdrawn | Web search and `ptc`: the code is still there but they are invisible in every case |
| Not in the capability selection layer | The administrator has not selected that capability for this agent |
| Filtered by form | Desktop mode hides target-management actions and session thread control / session yield |
| Filtered by a capability threshold | Without vision, image reading and desktop tools are filtered out; the multimodal set needs a matching model; web fetching / browser need configuration enabled first |
| Name short-circuited | When an alias and a skill share a name, the earlier route matches first |

## Naming and dynamic registration

- MCP tools are named "server @ tool"; a package-enabled server folds into a single tool operated through three actions: list, get detail, and call.
- When a tool name collides across servers, a server-name segment is appended; a collision within one server adds a runtime suffix.
- A skill name can be called directly; a knowledge base is **registered library by library**, and there is no single "knowledge base" tool.
- A user-defined tool alias enters the alias table and is **routed by alias first** during dispatch.

## Parallelism, timeouts, and budgets

| Item | Value |
| --- | --- |
| Parallelism ceiling | 4 |
| Parallel decision | Whether a batch of tool calls can run in parallel is decided in the orchestration layer |
| Lock semantics | A parallelizable tool takes a read lock; an exclusive tool takes a write lock |
| Default tool timeout | 120 seconds (minimum 1 second) |
| Tool-call budget | 10000 per round |
| Memory-recall budget | 2000 per round |

Tools outside the parallelism allowlist run serially, deliberately: write and execution tools are exclusive.

## Two clipping channels for results

| Channel | Behavior | Impact |
| --- | --- | --- |
| Display | Preview 2400 characters and 16 lines, with an ellipsis beyond that | Affects client rendering only; the original message and copied content stay complete |
| Model input | Truncate head and tail by budget; older reproducible observations go to micro-compaction | Decides what the model sees next |

The budget into the model: 10000 characters at each of head and tail; arrays at most 128 items (48 head + 16 tail); paged results 500 items (180 head + 60 tail). Clipping adds a "truncated" marker, so the model can see that content was cut.

Micro-compaction conventions: keep the most recent 5 groups, collapse older observations into a 768-byte preview, and only act when at least 256 tokens are saved.

## MCP integration

- An external MCP client must be configured with a name, endpoint, transport, request headers, authentication, allowed tools, whether to package, and tool specs.
- The hull's built-in MCP service endpoint is `/wunder/mcp` (streamable-http), exposing two tools, `wunder@excute` and `doc2md`; it returns 501 when the matching capability is off.
- The standalone Python FastMCP service carries its own configuration and toolset and is not part of this engine.

## The protocol follows the provider

The tool-call protocol is decided by the provider:

| Situation | Mode |
| --- | --- |
| The model explicitly configures a tool-call mode | Follow the configuration |
| The provider is openai | Text-protocol tool calls |
| Any other provider | Native function calls |
| An unrecognized value | Fall back to the tag / xml text protocol |

The text protocol's lark syntax is currently reserved for the patch form of text editing; parameter repair and nested restoration happen in the core library. There is no separate tool-protocol branch for Win7; the confirmed difference is only on the interop side — an older Win7 node does not declare the "execute command" capability.

## Removed and merged

| Status | Item |
| --- | --- |
| Removed | User-world tools, channel tools, `a2a_*` tools, `final_response`, `a2ui` tools, LSP query / node-call tools, and the standalone sleep-and-wait tool |
| Merged | `apply_patch` merged into **text editing** (one tool with three forms: replace / patch / subcommand) |
| Semantics changed | Session yield replaces the old sleep-and-wait |
| Temporarily withdrawn | Web search and `ptc` (code kept, invisible) |

## Common mistakes

- The four-stage model is not the main execution chain, and hooks are off by default.
- A clipped result does not mean the model cannot see it: the display channel and the model input channel are separate.
- More tools do not make the agent smarter; quite the opposite: many names and vague descriptions raise the error rate, so the default set is deliberately restrained.
- A tool error is not a failed call: a business failure is returned to the model as a normal result with a reason and a next-step hint.
- Not every tool can run in parallel: only reads on the parallelism allowlist can, while writes and executions are exclusive.
- Win7 has no tool-protocol fallback branch.

## Further reading

- [Tools (core concepts)](/docs/en/concepts/core-tools/) — the four-layer model and the default set
- [Tools overview](/docs/en/tools/) — parameter documentation for each tool
- [Context compression](/docs/en/concepts/core-context-compression/) — micro-compaction and history compression
- [Real-time behavior](/docs/en/concepts/core-realtime/) — how tool events are pushed to the client
- [Stability](/docs/en/concepts/core-stability/) — timeouts, budgets, and circuit breaking
- [Configuration reference](/docs/en/reference/config/) — MCP and tool toggle settings
