---
title: Form matrix
summary: How the program forms compare on tech stack, purpose, run model and OS support, plus which form to pick for your scenario.
updated_at: 2026-10-16
---

# Form matrix

wunder is a multi-form agent workbench. One engine is shipped as several program forms; the difference is only in the access layer.

| Icon | Form | Tech stack | Purpose | Run model | OS support |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 🚀 | wunder Server | Rust + axum 0.8; PostgreSQL | User and agent management platform (multi-tenancy, users and org units, agent app build and publish, gateway access and scheduling) | Long-running server process, Docker deployment, HTTP / WebSocket access | Linux / Docker |
| 🐉 | wunder Admin | Plain HTML + CSS + JavaScript (`web/`, no build step) | Server's admin console (four panel groups: system, agents, debug, docs) | Browser access, statically mounted by Server at `/` | Any browser |
| 🐝 | wunder Cloud | Vue 3 + TypeScript (Vite build, `frontend/`) | Cloud AI workbench (Server's user side) | Browser access; build output `frontend/dist` is served read-only by the nginx container (default port 18001) and is **not hosted by Server** | Any browser |
| 🍯 | wunder Desktop | Rust + Slint 1.18 (default frontend `crates/wunder-desktop/frontend/`, native UI, software rendering); SQLite | Local AI workbench (the flagship form, works out of the box) | Local desktop process, native in-process calls to the engine (no local network hop); Electron / Tauri shells are no longer maintained | Windows 7 x86 and up; Linux (ubuntu 18.04 baseline) x64 / arm64 (`.run` installer / `.portable`) |
| ⚙ | wunder CLI | Rust terminal TUI (ratatui + crossterm); SQLite | Manual local entry point (terminal, scriptable, automation) | Local terminal process, direct in-process engine calls, JSONL output supported | Windows 7 and up (GNU toolchain); Linux x64 / arm64 (ubuntu 18.04 sysroot) |

All forms share the same engine (wunder-runtime) and core (wunder-core): threads, tools, storage abstractions, realtime events and permission semantics are one body of code, and the difference is only in the access layer. Desktop and CLI do not depend on Server and can be installed and run on their own.

## Choosing a form

- **Individual users** → Desktop. The local AI workbench, the default entry point for personal use, ready to run after install.
- **Teams, or anyone working in a browser** → Cloud (web). Server's user side, provided with Server; once Server is deployed, members reach it in a browser.
- **Terminal and scripting** → CLI. The manual local entry point, terminal-driven, scriptable, automation-friendly.
- **Multi-user management and governance** → Server. Multi-tenancy, user and org-unit management, agent app build and publish, unified gateway access and scheduling. The admin console is the Admin (web), which starts with Server and is aimed at administrators.
