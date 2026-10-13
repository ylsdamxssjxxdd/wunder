---
title: Using the Helm (CLI)
summary: The manual local entry point. In a terminal it opens an interactive session; in a pipeline it degrades to a one-shot run. The full set of 22 slash commands, an exec --json event stream, and the complete cloud subcommand family; scheduled tasks and engine queueing are off on the Helm.
read_when:
  - You work terminal-driven, scripted, or in CI/CD
  - You need the cloud subcommands to operate device interlink from a shell
source_docs:
  - crates/wunder-cli/main.rs
  - crates/wunder-cli/runtime.rs
  - crates/wunder-cli/slash_command.rs
  - crates/wunder-cli/tui/mod.rs
  - crates/wunder-cli/exec_events.rs
  - crates/wunder-cli/args.rs
updated_at: 2026-10-13
---

# Using the Helm (CLI)

The Helm (CLI) is wunder's **manual local entry point**, an optional form: for daily use, install the Honeycomb (desktop); reach for the Helm when you work terminal-driven, scripted, or automated. It calls the engine in-process, reusing the same configuration, tools, and storage rather than standing up a separate execution path.

## When to use the Helm

| Scenario | Choose the Helm |
| --- | --- |
| Prefer working in a terminal | ✅ |
| Need scripting, automation, CI/CD | ✅ |
| Need piped output (JSONL) | ✅ |
| Need the `cloud` subcommands to operate device interlink | ✅ |
| Don't want to install the Honeycomb | ✅ |
| Need a graphical interface | ❌ Choose the Honeycomb |
| Need browser access | ❌ Choose the Beehive (ships with the Hull) |

## Install and launch

```bash
# Build (requires a Rust toolchain)
cargo build --release -p wunder-cli

# Run (no required arguments)
./target/release/wunder-cli
```

- **TTY** (stdin/stdout/stderr are all TTYs): opens the **interactive session** — a ratatui TUI rendered with an inline viewport (`Viewport::Inline`), so your shell scrollback is preserved
- **Non-TTY**: degrades to a **one-shot run**; pipelines and scripts take this path
- **Data directory**: `WUNDER_HOME` → `~/.wunder`
- **User configuration**: `~/.wunder/config.toml`

## Subcommands

| Subcommand | Alias | Action |
| --- | --- | --- |
| `exec` | `e` | Run a single agent task non-interactively; `--json` produces a JSONL event stream |
| `resume` | — | Resume a previous session |
| `tool` | — | Run built-in, MCP, or skill tools directly |
| `mcp` | — | Manage local MCP servers |
| `skills` | — | Manage the current user's local skills |
| `config` | — | View and change runtime configuration |
| `stress-thread` | — | Generate local synthetic threads for rendering stress tests |
| `doctor` | — | Diagnose the local runtime environment |
| `cloud` | — | Manage the local cloud connection and device interlink |
| `completion` | — | Generate shell completion scripts |

### The cloud subcommands

The full set for device interlink and cloud workspace operations:

```
cloud login | logout | status | models | refresh | logs
cloud devices                    # interlinked nodes for this account
cloud ws ls / cat / pull / push  # cloud workspaces
cloud threads / thread           # cloud thread list and detail
cloud send / watch               # drive threads through the interlink ledger
cloud audit / approve            # command ledger and approvals
```

### The exec --json event stream

One JSON object per line, with event names `thread` / `turn` / `item.*`. For pipelines and automation:

```bash
wunder-cli exec --json "tidy up the files in the current directory" | jq -c 'select(.type=="item.text")'
```

## Slash commands (22)

| Command | Alias | Action |
| --- | --- | --- |
| `/help` | `/h` | Help |
| `/status` | — | Current status |
| `/resume` | `/r` | Resume a previous session |
| `/new` | — | New thread |
| `/clear` | `/cls` | Clear the screen |
| `/model` | — | Switch model |
| `/permissions` | `/perm` / `/approvals` / `/approval` | Permissions and approvals |
| `/plan` | — | Plan panel |
| `/goal` | — | Goal-state management |
| `/edit` | — | Edit |
| `/init` | — | Initialize |
| `/attach` | — | Attachments |
| `/diff` | — | Patch diff |
| `/review` | — | Review |
| `/skills` | — | Skill list |
| `/rename` | — | Rename |
| `/compact` | — | Compress the conversation |
| `/mcp` | — | MCP management |
| `/config` | `/config show` is a sub-form | View and change configuration |
| `/exit` | — | Exit |
| `/quit` | `/q` | Exit |

There are **no** `cloud`, `cron`, `channel`, `memory`, or `knowledge` slash commands: `cloud` is a subcommand, and the other four have no management interface on the Helm.

## Keys

| Key | Action |
| --- | --- |
| `/` | Open the command palette |
| `Enter` | Send |
| `!` | Run shell directly |
| `Shift+Enter` / `Ctrl+J` | New line |
| `Tab` | Complete |
| `@` | File mention |
| `#` | Skill mention |
| `$` | Tool mention |
| `Ctrl+V` | Paste an image |
| Drag in an image or file | Attach |
| `Alt+,` / `Alt+.` | Lower / raise reasoning effort |
| `Ctrl+R` / `Ctrl+S` | Search input history |
| `Ctrl+G` | External editor |
| `Ctrl+O` | Copy the last reply |
| `Ctrl+T` | Full transcript |
| `Ctrl+L` | Clear the screen and start a new session |
| `Ctrl+C` | Exit |
| `Esc` | Interrupt the current turn; clear the draft when idle; close the shortcut panel |
| `F3` | View output and select a card |
| `Enter` (in the F3 panel) | Expand / collapse |
| `F2` | Run reminders |
| `F4` | Toggle mouse mode |
| `?` | Shortcut panel |
| `←` | Task center |

## First session

```bash
wunder-cli
> Write me a Hello World Python script
```

Seeing the model think → a batch of tools → results → a reply means it's working.

## What only the Helm can do

- The `exec --json` event-stream scripting contract
- The full set of `cloud` subcommands
- `doctor` local runtime diagnostics
- `stress-thread` local synthetic threads
- `completion` shell completions
- `tool` / `mcp` / `skills` run straight from the command line (without going through the agent main loop)

## What the Helm lacks

- **Scheduled tasks are explicitly off**: you can't add, modify, enable, or immediately run a scheduled task; viewing status, listing, deleting, and disabling still work — so you'll see old task lists, but the Helm won't schedule anything new.
- **Engine queueing is explicitly off too**: sending another message during a run doesn't join a queue; it hits the session lock outright and is judged "this user already has a running session," returning only once the busy window elapses. This is the opposite of the Honeycomb, which forces the queue open. To interject, queue, or rewrite the next message during a run, use the Honeycomb or the Beehive.
- **Channels, knowledge bases, long-term memory, and personas** have **no management interface** on the Helm
- **No voice input**: the Helm has no ASR; paste images with `Ctrl+V` or drag-and-drop
- **No interactive terminal UI**: the host is itself the terminal; shell access goes through the `!` prefix
- **No screenshot capture, tray, or global hotkeys**

## Runtime files

By default the Helm keeps runtime files under `~/.wunder`:

- Runtime configuration and SQLite state: `~/.wunder/cli/WUNDER_TEMP/`
- Engine configuration YAML: generated at `<temp>/config/wunder.yaml`
- User tools and skills: `~/.wunder/user_tools/` and `~/.wunder/skills/`
- Temporary-file API: `WUNDER_TEMP_DIR_ROOT` points to the same `~/.wunder/cli/WUNDER_TEMP/`

When a script needs an isolated runtime directory, override it with `--temp-root <path>`.

## Automation scenarios

- Calling `exec --json` in CI/CD pipelines to consume JSONL
- Batch tasks: an external scheduler looping over `exec`
- Pipelines: `cat input.txt | wunder-cli` triggers the non-TTY degradation
- Combining with `tool` / `mcp` / `skills` for pure tool runs
- **Scheduled tasks don't belong on the Helm**: it turns cron off; put them on the Honeycomb or Beehive scheduled-tasks page

## Common questions

**Graphical interface**: not supported; use the Honeycomb when you need a GUI.

**Scheduled tasks**: not supported; the Helm explicitly turns cron off.

**Pipelines**: supported. Non-TTY degrades to a one-shot exec; `--json` emits JSONL.

**Local environment check**: `wunder-cli doctor`.

## Further reading

- [Quick Start](/docs/en/start/quickstart/)
- [Surfaces Overview](/docs/en/surfaces/index/)
- [Core Concepts](/docs/en/concepts/)
- [Tools Overview](/docs/en/tools/)
- [First-Run Troubleshooting](/docs/en/start/troubleshooting-first-run/)
