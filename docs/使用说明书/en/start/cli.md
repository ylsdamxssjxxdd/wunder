---
title: CLI Usage
summary: The entry point for developers and automation. Terminal-driven, scriptable, JSONL output.
read_when:
  - You want to use wunder in the terminal
  - You want to integrate wunder into scripts or automation
source_docs:
  - wunder-cli/src/main.rs
updated_at: 2026-04-10
---

# CLI Usage

CLI is the entry point for developers and automation. Use it directly in the terminal — no GUI needed.

## When to Choose CLI

| Scenario | Choose CLI |
|----------|-----------|
| Prefer terminal operations | ✅ |
| Need scripting and automation | ✅ |
| Want to pipe output to other tools | ✅ |
| Don't want to install Desktop | ✅ |
| Need a graphical interface | ❌ Choose Desktop |

## Installation

```bash
# Build (requires Rust toolchain)
cargo build --release

# Run
./target/release/wunder-cli
```

## First Session

```bash
wunder-cli
> Write a Hello World Python script
```

You'll see: model thinks → calls tools → shows results → gives reply.

## Common Commands

| Command | Action |
|---------|--------|
| `/new` | Create new thread |
| `/stop` | Stop current execution |
| `/compact` | Compress current conversation |
| `/fork` | Fork current thread |
| `/resume` | Resume a previous thread |
| `/help` | Show help |
| `/quit` | Exit |

## TUI Interface

CLI includes a built-in TUI (Terminal User Interface), similar to Codex:

- Top: Conversation area
- Bottom: Input area
- Side: Thread list

## JSONL Output

CLI supports JSONL format output for piping and automation:

```bash
wunder-cli --format jsonl
```

Each line is a JSON object with event type and content.

## Automation Scenarios

CLI is great for:

- Calling agents in CI/CD pipelines
- Batch processing tasks
- Combining with other CLI tools
- Scheduled task triggers

## Next Steps

- [Quick Start](/docs/en/start/quickstart/)
- [Core Concepts](/docs/en/concepts/)
- [Tools Overview](/docs/en/tools/)
