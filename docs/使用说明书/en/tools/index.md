---
title: Tools Overview
summary: The current wunder tool system, the unified result envelope, state semantics, notable exceptions, and tool-selection guidance.
read_when:
  - You need to decide which category of tool to use first
  - You need to understand the latest tool result structure
source_docs:
  - src/services/tools.rs
  - src/services/tools/catalog.rs
  - src/services/tools/tool_error.rs
  - docs/工具返回内容优化表.md
updated_at: 2026-04-10
---

# Tools Overview

Read this tool set with the following mental model:

- First decide which class of tool fits the task
- Then check how that tool reports success or failure
- Finally confirm the core action and the minimum required arguments

## Start with this rule

wunder's built-in tools no longer return unrelated ad hoc formats. Most tools now converge on one shared envelope, and both the model and the frontend should read these top-level fields first:

- `ok`
- `action`
- `state`
- `summary`
- `data`

In practice, this means the business payload should usually be read from `data` first. `src/services/tools.rs` already provides compatibility helpers such as `tool_result_data()` and `tool_result_field()`, and the documentation follows that same convention.

## Unified success shape

Most built-in tools return the following on success:

```json
{
  "ok": true,
  "action": "tool_action",
  "state": "completed",
  "summary": "Human-readable summary.",
  "data": {
    "tool_specific": "payload"
  }
}
```

Some tools also include:

```json
{
  "next_step_hint": "What to do next"
}
```

### Common `state` values

- `completed`: the requested action finished
- `dry_run`: validation or rehearsal only, with no real side effects
- `accepted`: the task was accepted, but the final result is not ready yet
- `running`: execution is still in progress
- `yielded`: the current turn yielded control and has not produced a final answer
- `awaiting_input`: the frontend panel is open and waiting for user input
- `partial`: only part of the work finished and follow-up is still needed
- `timeout`: waiting timed out, but the target may not have failed permanently
- `noop`: the action was valid but did not change anything

## Unified failure shape

Most built-in tools return the following on failure:

```json
{
  "ok": false,
  "error": "Human-readable error message.",
  "sandbox": false,
  "data": {
    "tool_specific": "debug payload",
    "error_meta": {
      "code": "TOOL_EXAMPLE_ERROR",
      "hint": "What to fix before retrying.",
      "retryable": false,
      "retry_after_ms": null
    }
  },
  "error_meta": {
    "code": "TOOL_EXAMPLE_ERROR",
    "hint": "What to fix before retrying.",
    "retryable": false,
    "retry_after_ms": null
  }
}
```

When reading failures, prioritize:

1. `error_meta.code`
2. `error_meta.hint`
3. the context inside `data`

## Important exceptions to the unified envelope

These tools still need to be remembered individually.

### `final_response`

This is not a standard tool result. It is a very thin termination signal:

```json
{
  "answer": "The final reply to the user"
}
```

### `a2ui`

This tool sends structured UI instructions to the frontend, so it also keeps a thin wrapper:

```json
{
  "uid": "optional-surface-id",
  "a2ui": [ ... ],
  "content": "optional text"
}
```

### `schedule_task`

This tool currently uses a compact scheduling result rather than the unified `ok/action/state/summary/data` envelope. A common success result looks like:

```json
{
  "action": "add",
  "job": {
    "job_id": "job_xxx",
    "name": "Daily report reminder",
    "enabled": true,
    "schedule": {
      "kind": "every",
      "every_ms": 300000
    },
    "next_run_at": "2026-04-10T10:00:00+08:00",
    "last_run_at": null
  },
  "deduped": false
}
```

### `browser`

The browser tool mainly forwards browser-runtime results. Successful calls often include `ok: true`, but they do not always come wrapped in a unified `summary/data` shell. The returned fields vary significantly by action.

### `web_fetch`

`web_fetch` also returns the fetched result object directly on success rather than the unified success envelope:

```json
{
  "url": "https://example.com",
  "final_url": "https://example.com",
  "status": 200,
  "title": "Example",
  "content_type": "text/html",
  "content_kind": "html",
  "fetch_strategy": "direct_http",
  "format": "markdown",
  "extractor": "readability",
  "truncated": false,
  "cached": false,
  "fetched_at": "2026-04-10T03:00:00Z",
  "content": "..."
}
```

### `apply_patch`

`apply_patch` already uses the unified success envelope, but on failure it exposes its own patch-specific error codes and hints. Its error semantics are stricter than those of ordinary file tools.

## Current tool groups

## 1. Turn finishing and frontend coordination

- [Final Response](/docs/en/tools/final-response/)
- [Panels and a2ui](/docs/en/tools/panels-and-a2ui/)
- [Sleep and Yield](/docs/en/tools/sleep/): `sessions_yield` also belongs to turn-control semantics

## 2. Workspace and code

- [Workspace Files](/docs/en/tools/workspace-files/)
- [Execute Command](/docs/en/tools/exec/)
- [Apply Patch](/docs/en/tools/apply-patch/)
- [ptc](/docs/en/tools/ptc/)
- [LSP Query](/docs/en/tools/lsp/)
- [Skill Call](/docs/en/tools/skill-call/)
- [Read Image](/docs/en/tools/read-image/)

## 3. Web and desktop

- [Web Fetch](/docs/en/tools/web-fetch/)
- [Browser](/docs/en/tools/browser/)
- [Desktop Control](/docs/en/tools/desktop-control/)

## 4. Threads, subagents, and swarms

- [Thread Control](/docs/en/tools/thread-control/)
- [Subagent Control](/docs/en/tools/subagent-control/)
- [Agent Swarm](/docs/en/tools/agent-swarm/)

## 5. System links and memory

- [Self Status](/docs/en/tools/self-status/)
- [Memory Manager](/docs/en/tools/memory-manager/)
- [User World](/docs/en/tools/user-world/)
- [Channel Tool](/docs/en/tools/channel/)
- [A2A Tools](/docs/en/tools/a2a-tools/)
- [Node Invoke](/docs/en/tools/node-invoke/)
- [Schedule Task](/docs/en/tools/schedule-task/)
- [Sleep and Yield](/docs/en/tools/sleep/)

## Selection guidance

### Read the main content of a public webpage

Start with [Web Fetch](/docs/en/tools/web-fetch/). Do not jump straight to the browser.

### Click, type, or wait for dynamic rendering

Use [Browser](/docs/en/tools/browser/).

### Inspect local code

The usual sequence is:

1. Use [Workspace Files](/docs/en/tools/workspace-files/) to list directories or search first
2. Use [Workspace Files](/docs/en/tools/workspace-files/) again to read targeted ranges
3. Use [LSP Query](/docs/en/tools/lsp/) only when symbol-level understanding is needed

### Edit code

- For small and precise edits, use [Apply Patch](/docs/en/tools/apply-patch/)
- For full-file creation or replacement, use `write_file` from [Workspace Files](/docs/en/tools/workspace-files/)
- For compilation, tests, or scripts, use [Execute Command](/docs/en/tools/exec/)
- For a temporary Python helper, use [ptc](/docs/en/tools/ptc/)

### Coordinate multiple workers

- To launch temporary child runs inside the current session, use [Subagent Control](/docs/en/tools/subagent-control/)
- To dispatch other formal agents the user already owns, use [Agent Swarm](/docs/en/tools/agent-swarm/)
- To manage the main thread and branch threads, use [Thread Control](/docs/en/tools/thread-control/)

## What changed in this tool redesign

- Most built-in tools are now unified around `ok/action/state/summary/data`
- `data` is now the primary payload carrier
- Many tool schemas are noticeably tighter, with flatter inputs, explicit fields, and `additionalProperties: false`
- `subagent_control`, `thread_control`, and `agent_swarm` now follow a clear style: explicit action, explicit state, and explicit follow-up hints
- `schedule_task`, `browser`, `web_fetch`, `final_response`, and `a2ui` remain deliberate exceptions that must be remembered separately

## Next

- If your priority is reading and writing files, go straight to [Workspace Files](/docs/en/tools/workspace-files/)
- If your priority is collaboration between subagents and swarms, go straight to [Subagent Control](/docs/en/tools/subagent-control/) and [Agent Swarm](/docs/en/tools/agent-swarm/)
- If your priority is the exact return shape of one specific tool, jump to that tool's page for the latest success structure
