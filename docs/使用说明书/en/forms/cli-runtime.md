---
title: CLI Runtime
nav_title: Runtime
summary: What the CLI needs: a single binary, terminal and encoding requirements, the opt components it shares with Desktop, where its data lives, and the doctor check.
updated_at: 2026-11-14
---

# CLI Runtime

The CLI is the terminal entry to the same engine: one executable, no UI process, no local service. It is light, but it wants an interactive terminal with sane encoding, and script-facing abilities still need the supplement components.

## Parts

| Part | Location | Notes |
| --- | --- | --- |
| Program | `<root>/bin/wunder-cli` | `wunder-cli.exe` on Windows; `bin/wunder` is a compatibility symlink on Linux |
| Config | `<root>/config.toml` | The same engine config Desktop uses |
| Run state | `<root>/cli/WUNDER_TEMP/` | Sessions and SQLite |
| Workspace | The directory you start it from | `-C/--cd` points elsewhere |
| Components | `<root>/opt/` | Shared with Desktop, see [Desktop Runtime](/docs/en/forms/desktop-runtime/) |

A standalone build (`cargo build --release -p wunder-cli`) starts with an empty `opt/`: commands, Git, search and Python need the supplement imported separately.

## OS and terminal

- Windows 7 and up (GNU toolchain); Linux x64 / arm64 on an ubuntu 18.04 baseline.
- The terminal needs UTF-8 and wide-character support so Chinese and emoji stay aligned; a missing or minimal `TERM` degrades colors and borders to plain text.
- TTY decides the mode: with stdin / stdout / stderr all TTYs you get the interactive UI; redirected or piped, it degrades to a one-shot run, which is what scripts and `curl … |` use.
- No listening port and no background service; interlink tunnels and cloud subcommands are outbound connections.

## Self-check

`wunder-cli doctor` inspects the local runtime: install root, config readability, whether the supplement components are present, and whether the model endpoint answers. It is read-only and changes nothing.

When something fails, check in order: the resolved install root (a non-empty `WUNDER_HOME` wins), then whether `opt/<component>` exists, then whether the model config works.

## What it does not have

- No GUI, tray, or system notifications.
- No speech recognition; images come from paste or drag-and-drop.
- Knowledge bases are not implemented in the CLI, and long-term memory is not managed there.
