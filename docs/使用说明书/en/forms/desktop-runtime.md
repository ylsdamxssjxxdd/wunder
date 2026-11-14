---
title: Desktop Runtime
nav_title: Runtime
summary: What the desktop needs to run: an in-process engine, the opt components under the install root with their versions, system dependencies, storage and ports.
updated_at: 2026-11-14
---

# Desktop Runtime

Desktop puts the engine in the same process: UI, agent scheduling and tool execution all run locally, with no local HTTP/WebSocket hop and no separate service. So its runtime is two things — the program itself and the bundled components under the install root.

## What it is made of

| Part | Location | Role |
| --- | --- | --- |
| Program | `<install root>/app/desktop` | Slint UI plus engine, with its own runtime libraries |
| Launcher | `<install root>/bin/wunder-desktop` | Exports `WUNDER_HOME` and starts the program |
| Bundled components | `<install root>/opt/` | Python, Git, ripgrep, FFmpeg, 7-Zip, browser, fonts |
| Local state | `<install root>/cli/WUNDER_TEMP/` | Sessions, tool logs and SQLite |

## Bundled components

Desktop and CLI share one `opt/`, so installing Desktop also provides the CLI runtime.

| Component | Version | Used for |
| --- | --- | --- |
| `opt/python` | 3.8.10 (Windows); 3.8.18 (Linux) | Agent-written scripts and Python tools |
| `opt/git` | MinGit 2.46.2 (Windows) / 2.46.2 built from source (Linux) | Clone, patches, repository work |
| `opt/rg` | 14.1.0 (Windows); 14.1.1 (Linux) | Content search and terminal search |
| `opt/ffmpeg` | static 4.4.1 | Frame and audio extraction, transcoding (no `ffprobe`) |
| `opt/7zip` | 25.01 | Archive packing and unpacking |
| `opt/browser` | bundled Chrome 109 (win32) on Windows; Chrome for Testing 122 (amd64) or Chromium 120 (arm64) on Linux | Offline engine for the browser tool |
| `opt/fonts` | Office fonts plus short-video fonts | Document layout, drawing, video rendering |
| `opt/powershell` | PowerShell 7 (Windows only) | Command execution |

Components follow the supplement, not the app version. Moving machines means copying the whole install root; `opt/` travels with it.

## System dependencies

- Windows 7 x86 and up; Linux with ubuntu 18.04 as the baseline (glibc 2.27). Older glibc will not do; newer distributions are fine.
- The UI and its libraries ship inside `app/desktop`; the system needs no GTK, Python or browser of its own.
- Installing the `.run` needs `xz` (present by default on ubuntu 18.04 and later; minimal images need `xz-utils`). Running afterwards does not.

## Database, ports and network

- State lives in local SQLite; there is no external database and no database service to back up.
- Nothing listens on any port by default, and there is no externally reachable admin address. Cross-machine features go through the interlink tunnel or a cloud workspace.
- Calling a model, fetching a page or reaching a drive is a one-off outbound request, not a resident port.

## Checking it after install

1. Settings opens the tool manager, which means the component directory was found.
2. Ask for `python -V` or `git --version` in a chat; matching the table above means the path is wired.
3. If it will not run, check which install root the process used — a non-empty `WUNDER_HOME` wins over the install-root marker.

`wunder-cli doctor` does a deeper check; see [CLI Runtime](/docs/en/forms/cli-runtime/).
