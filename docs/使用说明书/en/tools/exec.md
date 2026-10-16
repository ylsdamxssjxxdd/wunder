---
title: Execute Command
summary: Arguments for `execute_command`, the blocking and background execution modes, polling and writing in a command session, output guards and budgets, and failure diagnosis.
updated_at: 2026-10-16
---

# Execute Command

`execute_command` runs shell commands, compiles, tests, calls an existing command-line tool, or runs a throwaway program such as a temporary Python script. To change files only slightly, use `Text Edit` (patch form); to just read code, use [Workspace Files](/docs/en/tools/workspace-files/).

## Arguments

Give the command to run, and you can also specify a working directory. Common arguments:

- **Timeout** `timeout_s`: 120 seconds by default; pass 0 for no limit.
- **Background** `run_in_background`: off by default, meaning the call blocks until the command finishes.
- **Yield wait** `yield_time_ms`: used only together with background mode.
- **Budget** `budget`: caps time, output bytes, and the maximum number of commands.
- **Preview** `dry_run`: validates the plan without actually running it.

## Blocking and background

Execution blocks by default: one call waits until the command exits or the timeout is reached, then returns the full output at once; on timeout the process is killed and the partial output collected so far is returned.

Set `run_in_background` only for long-running commands: after a brief wait, a command that is still running returns immediately, with a next-step hint in the result. Track a background command afterwards with the command session below; do not spin in an idle loop waiting for it.

Only a bounded preview of the output is kept. Cancelling the current thread terminates the active commands of that thread and of its child threads.

## Command session

A long-lived background command session is managed by the `command_session` tool, which supports switching between foreground and background: a foreground call reads the current status and output directly, while switching to background leaves the command running.

- With no input given, a call is purely a poll that reads back the session's status and the output collected so far.
- To feed content into the session, write it with `write_stdin`.
- With `yield_time_ms` (up to 60000), one call can keep waiting until the command exits.

## Automatic behavior

- **Output guard**: command output over the threshold is trimmed, keeping a preview within a bounded range and noting in the result that the output was truncated.
- **Budget**: time, output bytes, and the maximum number of commands can all be capped, so a single command cannot eat too many resources.
- **Patch handoff**: if the content passed in is not a command but a complete `*** Begin Patch ... *** End Patch` block, the system hands it to the patch form of `Text Edit` and marks on the result that this call was intercepted from `execute_command`.

## Errors

- A non-zero exit, a timeout, a working directory that is missing or not a directory, a disallowed command, or exceeding the command budget all fail.
- On a timeout or a non-zero exit, the collected output is still kept in the result, so you can keep diagnosing.
- The administrator event details add up to 4 diagnostics, each with the command, its index, the exit code, and a bounded tail of output; the failure message sent to the model is compressed into one error line plus a next-step hint.
