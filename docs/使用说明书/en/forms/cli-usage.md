---
title: CLI Usage
nav_title: Usage
summary: CLI usage: subcommands, slash commands and keys, plus what only the CLI can do.
updated_at: 2026-11-14
---

# CLI Usage

## Usage

### Subcommands

| Subcommand | Alias | What it does |
| --- | --- | --- |
| `exec` | `e` | run one agent task non-interactively; `--json` gives a JSONL event stream |
| `resume` | — | resume a previous session |
| `trajectory` | — | view, search and export thread trajectories (JSON, bidirectional cursors) |
| `tool` | — | run a built-in / MCP / skill tool directly |
| `mcp` | — | manage local MCP servers |
| `skills` | — | manage the current user's local skills |
| `config` | — | read and change runtime configuration |
| `stress-thread` | — | generate a synthetic local thread for rendering stress |
| `doctor` | — | diagnose the local runtime |
| `cloud` | — | manage the local cloud connection and device interlink |
| `completion` | — | generate shell completion |

### Slash commands

22 in total: `/help` `/status` `/resume` `/trajectory` `/new` `/clear` `/model` `/permissions` `/plan` `/goal` `/edit` `/init` `/attach` `/diff` `/review` `/skills` `/rename` `/compact` `/mcp` `/config` `/exit` `/quit`.

There are **no** `cloud`, `cron`, `channel`, `memory` or `knowledge` slash commands: `cloud` is a subcommand, and the other four have no management UI in the CLI.

### Keys

| Key | Action |
| --- | --- |
| `/` | open the command palette |
| `Enter` | send |
| `!` | run a shell command |
| `Shift+Enter` / `Ctrl+J` | newline |
| `Tab` | completion |
| `@` / `#` / `$` | mention a file / skill / tool |
| `Ctrl+V`, drag and drop | attach an image or file |
| `Alt+,` / `Alt+.` | lower / raise reasoning effort |
| `Ctrl+R` / `Ctrl+S` | search input history |
| `Ctrl+G` | external editor |
| `Ctrl+O` | copy the last reply |
| `Ctrl+T` | full transcript |
| `Ctrl+L` | clear the screen and start a new session |
| `Esc` | interrupt the current turn; clear the draft when idle |
| `F4` | switch mouse mode (capture ↔ native) |
| `←` | task center |
| `?` | shortcut panel |
| `Ctrl+C` | quit |

## CLI-only and missing

- **Only the CLI**: the `exec --json` script contract, the full `cloud` subcommand set, `doctor`, `stress-thread`, `completion`, and running `tool` / `mcp` / `skills` outside the agent loop.
- **Not in the CLI**: scheduled tasks are switched off (old tasks stay visible, new ones are never queued); channels, knowledge base, long-term memory and companions have no management UI; no voice input, no screenshot capture, no tray or global hotkey; the host is already a terminal, so there is no interactive terminal panel.
