---
title: Execute Command
summary: Arguments for `execute_command`, the blocking and background execution modes, polling and writing in a command session, output guards and budgets, and failure diagnosis.
updated_at: 2026-11-12
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

One call has exactly two intents, decided by `input`:

- **Empty `input`** (or omitted): read back status plus new output only, returning within 250 ms by default — the cheap "take a look" call.
- **Non-empty `input`**: write it to the session's stdin first; that call then waits for the command to exit within the same wait window (the `printf ... | cmd` semantics). Pass `wait_for=output` to return as soon as output appears. Sessions started without an interactive stdin reject the write, say it was not delivered, and suggest piping the input through the command itself (`printf '...' | cmd`).
- **Ctrl-C `input`**: treated as an interrupt, i.e. terminate the command; works even when the session has no stdin.

`wait_for` defaults to `output` (return as soon as there is new output). Pass `wait_for=exit` to ignore intermediate output and block until the command exits or the wait window ends; the window is `yield_time_ms` (up to 60000, and 60000 is the default in that mode).

An already-exited session is not an error: its final output and exit code are still returned. Decide completion from `status` and `exit_code` instead of re-running the command.

## Automatic behavior

- **Output guard**: command output over the threshold is trimmed, keeping a preview within a bounded range and noting in the result that the output was truncated.
- **Budget**: time, output bytes, and the maximum number of commands can all be capped, so a single command cannot eat too many resources.
- **Patch handoff**: if the content passed in is not a command but a complete `*** Begin Patch ... *** End Patch` block, the system hands it to the patch form of `Text Edit` and marks on the result that this call was intercepted from `execute_command`.

## Errors

- A non-zero exit, a timeout, a working directory that is missing or not a directory, a disallowed command, or exceeding the command budget all fail.
- On a timeout or a non-zero exit, the collected output is still kept in the result, so you can keep diagnosing.
- The administrator event details add up to 4 diagnostics, each with the command, its index, the exit code, and a bounded tail of output; the failure message sent to the model is compressed into one error line plus a next-step hint. That summary keeps the first and last 3 lines and reports total and omitted line counts instead of keeping only the tail; exit code 141 or a broken-pipe stderr is called out as a downstream head/cut closing the pipe (SIGPIPE), not a command failure.

## Shell

Commands run in the active shell, and the tool description names it, so the model can pick the right syntax:

- **Windows desktop** prefers the bundled portable `pwsh` (PowerShell 7). If that probe fails it falls back to the system `powershell.exe` (2.0 on Windows 7, 5.1 on Windows 10/11), and only then to `cmd.exe`. Even without the supplement bootstrap, the runtime discovers `opt/powershell/pwsh.exe` from the shared home and program directories, so a system shell found first cannot displace the bundled one; a slow first cold start is retried with the bundled host preferred.
- **Linux / macOS desktop** always uses bash with `set -o pipefail`, so a failing middle stage of a pipeline is not masked by a successful last command.
- **Android** uses `/system/bin/sh` (mksh).

The bundled pwsh becomes available as soon as the supplement is imported, with no app restart. Settings can override the choice: `cmd` forces cmd.exe, and any other executable becomes a PowerShell host candidate.

When no usable PowerShell is detected but a `pwsh.exe` is present, the tool description says so explicitly ("this shell is cmd.exe, call `pwsh` when you need PowerShell") instead of letting the model believe the machine has none.
