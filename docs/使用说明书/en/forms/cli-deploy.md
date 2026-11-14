---
title: CLI Deployment
nav_title: Deployment
summary: CLI deployment: OS requirements, how to get it, startup arguments and runtime files.
updated_at: 2026-11-14
---

# CLI Deployment

## Deployment

- **OS**: Windows 7 and up (GNU toolchain); Linux x64 / arm64 (ubuntu 18.04 sysroot).
- **Getting it**: installed together with the Desktop package, or build it with `cargo build --release -p wunder-cli`.
- **Startup**: no required arguments. A **TTY** (stdin, stdout and stderr all TTYs) opens the interactive interface; anything else falls back to a one-shot run, which is what pipes and scripts get.
- **Working directory**: the directory you start it in; `-C/--cd` points elsewhere.
- **Data**: `WUNDER_HOME` → `~/.wunder`; user configuration in `~/.wunder/config.toml`; engine configuration `~/.wunder/config/wunder.yaml` is shared with the Desktop. `--temp-root` isolates the run directory.
- `wunder-cli doctor` checks whether the local runtime is ready.
