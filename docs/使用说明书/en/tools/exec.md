---
title: Execute Command
summary: Arguments for `execute_command`, the blocking and background execution modes, polling and writing in a command session, output guards and budgets, and failure diagnosis.
updated_at: 2026-11-18
---

# Execute Command

`execute_command` runs shell commands, compiles, tests, calls an existing command-line tool, or runs a throwaway program such as a temporary Python script. To change files only slightly, use `Text Edit` (patch form); to just read code, use [Workspace Files](/docs/en/tools/workspace-files/).

## Arguments

Give the command to run, and you can also specify a working directory. Common arguments:

- **Command** `content`: required, the command itself.
- **Working directory** `workdir`: optional, a workspace-relative path.
- **Wait window** `yield_time_ms`: 10000 by default, 50–30000. It only decides how long this call waits before handing control back; it is not a kill timeout.
- **Action label** `description`: a short title shown in the UI.

## Blocking and background

A call first waits up to `yield_time_ms`: a command that finishes sooner returns its full result (exit code, stdout, stderr) immediately; one that is still running at the deadline returns `command_session_id`, the current `seq` and the output so far, and keeps running. The deadline hands control back without killing the process, so long commands need no separate "background" flag.

Only a bounded preview of the output is kept. Cancelling the current thread terminates the active commands of that thread and of its child threads.

## Command session

A long-lived background command session is managed by the `command_session` tool, which supports switching between foreground and background: a foreground call reads the current status and output directly, while switching to background leaves the command running.

Without `command_session_id` the call is a read-only task list: it lists this thread's command tasks (id, status, elapsed time, current `seq`, output tail) so you can confirm whether a command is actually running instead of guessing with `ps`, and it changes no session.

One call has exactly three intents, decided by `chars`:

- **Empty `chars`** (or omitted): read back status plus new output only, waiting at most `yield_time_ms` (1000 ms by default) and returning immediately when new output arrives.
- **Non-empty `chars`**: write those bytes to the session's stdin first, then read new output within the same wait window; include a newline when answering a line-based prompt. Sessions started without an interactive stdin reject the write, say it was not delivered, and suggest piping the input through the command itself (`printf '...' | cmd`).
- **Ctrl-C `chars`**: treated as an interrupt, i.e. terminate the command; works even when the session has no stdin, and the result is marked `interrupted` with the output produced so far.

Optional arguments:

- **`yield_time_ms`**: wait ceiling for this call. A plain poll defaults to 1000 with a 30000 ceiling; `wait_for=output` defaults to 30000 and `wait_for=exit` to 60000, both up to 300000 (5 minutes). Pass 0 for an immediate read.
- **`after_seq`**: return only output newer than that `seq`, for catching up on a chunk that was truncated or only partially shown.
- **`wait_for=exit`**: ignore intermediate output and block until the process exits or the window ends. Minute-long builds and renders finish within one or two calls this way.
- **`wait_for=output`**: return as soon as there is new output (or the process exits), for watching progress instead of waiting for the end.

A second call of an identical command while it is still running in the same thread reuses that session (the result carries `reused=true`) instead of starting a second copy. This matters for programs with a single-instance lock, where the rerun would only be blocked by the first one.

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
