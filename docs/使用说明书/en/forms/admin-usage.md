---
title: Admin Usage
nav_title: Usage
summary: Admin usage: what each panel group does and the operating rules.
updated_at: 2026-11-14
---

# Admin Usage

## Usage

- **System**: process health, locks and queues, runtime metrics; hot-update system settings; edit and push system messages; maintain the org tree (4 levels deep), accounts, quotas and online state; view cross-user threads (read-only); upload update packages and publish the version manifest.
- **Agents**: configure LLM / TTS / image / video models; build preset agent templates and declare which fields users may customize; manage global companion packs; keep built-in tools, MCP, knowledge base, skills and tool visibility in one place; maintain system prompt templates.
- **Debug**: thread list plus details and trajectory; throughput test (input 1024 to 1 MiB, output 1024 to 8192, concurrency up to 1024); performance test (fixed test user and separate data root, 5-second command timeout); capability evaluation scoring; direct image / video / transcription / speech-synthesis tests.
- **Docs**: the whole manual embedded in the site, plus the API docs entry.
- Key actions carry explicit state, error feedback and confirmation boundaries; a frontend confirmation never replaces backend authorization.

## Boundaries

- **It does not chat**: it is a governance and debugging backend, and ordinary users never see it.
- **No user-level scheduled tasks, channel management, long-term memory or interlink governance panels**: schedules and channels exist only as preset-agent templates.
- **Thread management is read-only**: you can see other users' threads and states, but there is no goal editor; goals live on the user side (Cloud timeline, Desktop composer, CLI `/goal`).
