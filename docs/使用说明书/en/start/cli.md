---
title: Using the Helm (CLI)
summary: The manual local entry point. An interactive session on a TTY and a one-shot run off it; 11 subcommands, the full set of 22 slash commands, an exec --json event stream, and the complete cloud subcommand family; scheduled tasks and engine queueing are off on the Helm.
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
updated_at: 2026-10-16
---

# Using the Helm (CLI)

The manual local entry point, an optional form. For daily use, install the Pupa (desktop); reach for the Helm when you work terminal-driven, scripted, or automated. It calls the engine in-process, reusing the same configuration, tools, and storage rather than standing up a separate execution path.

## Prerequisites

| Scenario | Choose the Helm |
| --- | --- |
| Prefer working in a terminal | ✅ |
| Need scripting, automation, CI/CD | ✅ |
| Need piped output (JSONL) | ✅ |
| Need the `cloud` subcommands to operate device interlink | ✅ |
| Don't want to install the Pupa | ✅ |
| Need a graphical interface | ❌ Choose the Pupa |
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

Your first session:

```bash
wunder-cli
> Write me a Hello World Python script
```

Seeing the model think → a batch of tools → results → a reply means it's working. `wunder-cli doctor` checks whether the local runtime environment is ready.

## Subcommands

| Subcommand | Alias | Action |
| --- | --- | --- |
| `exec` | `e` | Run a single agent task non-interactively; `--json` produces a JSONL event stream |
| `resume` | — | Resume a previous session |
| `trajectory` | — | Inspect, search, and export thread trajectories; JSON and bidirectional cursors |
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

## Slash commands

22 in total:

| Command | Alias | Action |
| --- | --- | --- |
| `/help` | `/h` | Help |
| `/status` | — | Current status |
| `/resume` | `/r` | Resume a previous session |
| `/trajectory` | `/trace` | Inspect the current or specified thread trajectory |
| `/new` | — | New thread |
| `/clear` | `/cls` | Clear the screen |
| `/model` | — | Switch model; `/model add` registers an endpoint |
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
| `/config` | `/config show` is a sub-form | Show the effective config; `/config edit` opens config.toml |
| `/exit` | — | Exit |
| `/quit` | `/q` | Exit |

There are **no** `cloud`, `cron`, `channel`, `memory`, or `knowledge` slash commands: `cloud` is a subcommand, and the other four have no management interface on the Helm.

## Trajectory logs

Run `/trajectory` (alias `/trace`) in the interactive session to inspect the current thread, or `/trajectory <session_id>` for another thread. In the task center, press `t` to inspect the selected thread without switching the executing thread.

The viewer provides sequence / elapsed-time controls, turn and tool-group folding, zoom, three input/model/tool lanes, a ledger, and item details. Wide terminals show details beside the ledger; press `i` for a separate detail page on narrow terminals.

| Key | Trajectory action |
| --- | --- |
| `↑↓` / `j k`, `PageUp PageDown` | Select records; automatically read older / newer records at window edges |
| `Enter` | Inspect the selected user turn; inside a turn, toggle detail focus |
| `i` / `→` | Toggle detail focus; arrows and page keys scroll focused details |
| `Tab` / `Shift+Tab` | Cycle applicable overview, preview, raw, params, result, schema, and timing tabs |
| `h` / `l` in details | Pan long lines horizontally |
| `d`, `+ -`, `0` | Sequence / elapsed time, zoom around selection, reset zoom |
| `t`, Space, `c` | Fold all turns, selected turn, or tools in a model round |
| `/` | Enter a full-text query; up to 20 hits per request, refine if more are available |
| `f`, `r` | Toggle live follow, refresh / retry; manual browsing pauses follow |
| `y`, `e` | Copy the current detail tab, export full JSONL to the launch directory |
| `Esc` / `q`, `?` | Return one level, show help |

The viewer uses shared engine projections and body reads in the background. It retains a maximum of 600 ledger rows, with bidirectional cursors to reread evicted records. Detail previews are limited to 1 MiB and 16,384 lines per tab with an explicit notice; export retrieves the full log. Exports retain user-visible records; system prompt contents remain hidden.

For scripts and pipelines, use the subcommand. Without a thread ID it selects the most recent thread in the current workspace:

```bash
wunder-cli trajectory --raw
wunder-cli trajectory <session_id> --turn <root_turn_id> --limit 80 --raw
wunder-cli trajectory <session_id> --before <cursor> --raw
wunder-cli trajectory <session_id> --after <cursor> --raw
wunder-cli trajectory <session_id> --search <query> --raw
wunder-cli trajectory <session_id> --item <item_id>
wunder-cli trajectory <session_id> --export trajectory.jsonl
wunder-cli trajectory <session_id> --export -
```

Use returned `next_before` / `next_after` cursors; `--before` and `--after` are mutually exclusive. `--raw` or global `--json` emits structured data. Export destinations must not already exist. A final `export_complete` record distinguishes a complete export from an interrupted file.

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
| Mouse click | Select a card; click its Show details / Show less row to fold or unfold; click in the composer to place the caret |
| Mouse drag / double click / triple click | Drag copies the marked rows, a double click takes the word under the pointer, a triple click the whole line; right click re-copies whatever is still marked |
| `Ctrl` + click a link | Open that http/https/file href with the system's default program |
| `F4` | Toggle mouse mode: capture (app handles clicks and the wheel) ↔ native (the terminal keeps selection, copy and its own wheel) |
| `?` | Shortcut panel |
| `←` | Task center |

## Capability differences from the other interfaces

### Only the Helm has these

- The `exec --json` event-stream scripting contract
- The full set of `cloud` subcommands
- `doctor` local runtime diagnostics
- `stress-thread` local synthetic threads
- `completion` shell completions
- `tool` / `mcp` / `skills` run straight from the command line (without going through the agent main loop)

### The Helm lacks these

- **Scheduled tasks are explicitly off**: you can't add, modify, enable, or immediately run a scheduled task; viewing status, listing, deleting, and disabling still work — so you'll see old task lists, but the Helm won't schedule anything new.
- **Engine queueing is explicitly off too**: sending another message during a run doesn't join a queue; it hits the session lock outright and is judged "this user already has a running session," returning only once the busy window elapses. This is the opposite of the Pupa, which forces the queue open. To interject, queue, or rewrite the next message during a run, use the Pupa or the Beehive.
- **Channels, knowledge bases, long-term memory, and personas** have **no management interface** on the Helm
- **No voice input**: the Helm has no ASR; paste images with `Ctrl+V` or drag-and-drop
- **No interactive terminal UI**: the host is itself the terminal; shell access goes through the `!` prefix
- **No screenshot capture, tray, or global hotkeys**

## Runtime files

By default the Helm keeps runtime files under `~/.wunder`:

- Runtime configuration and SQLite state: `~/.wunder/cli/WUNDER_TEMP/`
- Engine configuration YAML: `~/.wunder/config/wunder.yaml`, the same file the Pupa uses; the Helm reads it back, projects its own layers onto it and writes it out, so the model list is visible to both
- User tools and skills: `~/.wunder/user_tools/` and `~/.wunder/skills/`
- Temporary-file API: `WUNDER_TEMP_DIR_ROOT` points to the same `~/.wunder/cli/WUNDER_TEMP/`

When a script needs an isolated runtime directory, override it with `--temp-root <path>`.

## Automation scenarios

- Calling `exec --json` in CI/CD pipelines to consume JSONL
- Batch tasks: an external scheduler looping over `exec`
- Pipelines: `cat input.txt | wunder-cli` triggers the non-TTY degradation
- Combining with `tool` / `mcp` / `skills` for pure tool runs
- **Scheduled tasks don't belong on the Helm**: it turns cron off; put them on the Pupa or Beehive scheduled-tasks page
