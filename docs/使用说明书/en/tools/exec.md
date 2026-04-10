---
title: Execute Command
summary: The budgets, dry-run behavior, output guards, and return structure of `execute_command`.
read_when:
  - You need to run shell commands, compile, test, or invoke an existing CLI
source_docs:
  - src/services/tools.rs
updated_at: 2026-04-10
---

# Execute Command

`execute_command` is no longer just "send shell text and read stdout." Its current design centers on three changes:

- it supports `dry_run`
- it enforces execution budgets and output guards
- it automatically intercepts patch text and reroutes it to `apply_patch`

## Minimum arguments

```json
{
  "content": "cargo check --release"
}
```

## Common arguments

- `content`
- `workdir`
- `timeout_s`
- `budget`
- `dry_run`

Common fields inside `budget`:

- `time_budget_ms`
- `output_budget_bytes`
- `max_commands`

## Success result

```json
{
  "ok": true,
  "action": "execute_command",
  "state": "completed",
  "summary": "Executed 1 commands.",
  "data": {
    "results": [
      {
        "command": "cargo check --release",
        "command_index": 0,
        "command_session_id": "cmd_xxx",
        "returncode": 0,
        "stdout": "...",
        "stderr": "",
        "output_meta": {
          "truncated": false,
          "total_bytes": 1024,
          "omitted_bytes": 0,
          "stdout": { "truncated": false },
          "stderr": { "truncated": false }
        }
      }
    ],
    "budget": {
      "time_budget_ms": 60000,
      "output_budget_bytes": 32768,
      "max_commands": 4
    },
    "output_guard": {
      "truncated": false,
      "commands": 1,
      "truncated_commands": 0,
      "total_bytes": 1024,
      "omitted_bytes": 0,
      "effective_total_bytes": 32768
    },
    "sandbox": false
  }
}
```

If output was trimmed by the guard, the result may also include:

```json
{
  "next_step_hint": "Command output was truncated by the output guard..."
}
```

## `dry_run`

```json
{
  "ok": true,
  "action": "execute_command",
  "state": "dry_run",
  "summary": "Validated command plan without execution.",
  "data": {
    "dry_run": true,
    "workdir": "C:/.../workspace",
    "command_count": 1,
    "commands": ["cargo check --release"],
    "timeout_s": 60,
    "budget": { ... },
    "output_guard": { ... },
    "sandbox": false
  }
}
```

## Failure results

Non-zero exits, timeouts, and bad working directories all use the unified failure envelope. Common error codes include:

- `TOOL_EXEC_COMMAND_REQUIRED`
- `TOOL_EXEC_WORKDIR_NOT_FOUND`
- `TOOL_EXEC_WORKDIR_NOT_DIR`
- `TOOL_EXEC_NOT_ALLOWED`
- `TOOL_EXEC_TIMEOUT`
- `TOOL_EXEC_NON_ZERO_EXIT`
- `TOOL_EXEC_BUDGET_COMMAND_LIMIT`

On timeouts or non-zero exits, `data.results` still keeps the collected stdout and stderr so the model can continue diagnosing the problem.

## Special behavior: patch text sent by mistake

If `content` is not a command but a complete `*** Begin Patch ... *** End Patch` block, the system automatically redirects execution to `apply_patch` and adds:

```json
{
  "intercepted_from": "execute_command"
}
```

## When not to use it

- If you only need a small file edit, use [Apply Patch](/docs/en/tools/apply-patch/)
- If you need a temporary Python helper, use [ptc](/docs/en/tools/ptc/)
- If you only want to read code, use [Workspace Files](/docs/en/tools/workspace-files/)
