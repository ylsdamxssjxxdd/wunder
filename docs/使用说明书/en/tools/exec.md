---
title: Execute Command
summary: Run shell commands, compile, test, or call an existing command-line tool; it supports previews, resource budgets, and output guards.
updated_at: 2026-10-13
---

`execute_command` runs shell commands, compiles, tests, or calls an existing command-line tool. It supports a preview (`dry_run`), carries command budgets and output guards, and automatically hands patch text that arrives by mistake over to `Text Edit`.

## When to use it

- You need to run a compile, test, build, or script
- You need to call an existing command-line tool
- You need to run a throwaway program, such as a temporary Python script

To change files only slightly, use `文本编辑` (patch form); to just read code, use [Workspace Files](/docs/en/tools/workspace-files/).

## How to use it

Give the command to run, and you can also specify a working directory. Common arguments:

- **Timeout** `timeout_s`: 120 seconds by default; pass 0 for no limit.
- **Background** `run_in_background`: off by default, meaning the call blocks until the command finishes.
- **Yield wait** `yield_time_ms`: used only together with background mode.
- **Budget** `budget`: caps time, output bytes, and the maximum number of commands.
- **Preview** `dry_run`: validates the plan without actually running it.

## Blocking or background

Execution blocks by default: one call waits until the command exits or the timeout is reached, then returns the full output at once; on timeout the process is killed and the partial output collected so far is returned.

Set `run_in_background` only for long-running commands: after a brief wait, a command that is still running returns immediately, with a next-step hint in the result. Poll it afterwards with a command session: `yield_time_ms` can go up to 60000, letting one call keep waiting until the command exits — do not spin in an idle loop waiting for a background command. Empty input only polls; when input is needed, write it with `write_stdin`.

Only a bounded preview of the output is kept. Cancelling the current thread terminates the active commands of that thread and of its child threads.

## What it does automatically

- **Output guard**: command output over the threshold is trimmed, keeping a preview within a bounded range and noting in the result that the output was truncated.
- **Budget**: time, output bytes, and the maximum number of commands can all be capped, so a single command cannot eat too many resources.
- **Patch handoff**: if the content passed in is not a command but a complete `*** Begin Patch ... *** End Patch` block, the system hands it to the patch form of `Text Edit` and marks on the result that this call was intercepted from `execute_command`.

## Diagnosing failures

- A non-zero exit, a timeout, a working directory that is missing or not a directory, a disallowed command, or exceeding the command budget all fail.
- On a timeout or a non-zero exit, the collected output is still kept in the result, so you can keep diagnosing.
- The administrator event details add up to 4 diagnostics, each with the command, its index, the exit code, and a bounded tail of output; the failure message sent to the model is compressed into one error line plus a next-step hint.
