---
title: Desktop Deployment
nav_title: Deployment
summary: Desktop deployment: OS requirements, installer packages, install root and local data.
updated_at: 2026-11-14
---

# Desktop Deployment

## Deployment

### OS and installer

- **OS**: Windows 7 x86 and up; Linux (ubuntu 18.04 baseline) x64 / arm64; modern Windows / Linux works too.
- **Packages**: `wunder-desktop-*-win7-x86-setup.exe`, `wunder-desktop-*-linux-*.run` (Desktop, `wunder-cli` and the runtime in one pass, no FUSE or admin rights needed), plus `.tar.gz` and `.portable`.
- **Install root**: `~/.wunder` by default (Linux `--home=PATH`, changeable in the Windows wizard); the executable lands in `bin/wunder-desktop`.
- **Menu and commands**: the installer adds `bin` to PATH on Windows or links it into `~/.local/bin` on Linux, so `wunder-cli` and `wunder-desktop` work from a new terminal.
- No port and no required arguments; the bundled supplement installs the browser, Python 3, Git, rg and ffmpeg.

### Local data

| Content | Location |
| --- | --- |
| Root | the install root (`WUNDER_HOME`; `~/.wunder` by default, marked by `.wunder-home`) |
| Workspace | `<root>/workspace` |
| Desktop settings | `<temp>/config/desktop.settings.json` |
| Sessions and tool logs | local SQLite |

Moving to another machine means copying the whole install root. On startup it scans the work tree (up to 4096 items) and restores tasks that never finished. "Local first" means data stays on your machine; it does not stop you from calling remote models or web pages.
