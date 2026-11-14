---
title: wunder
summary: A general-purpose AI operating system
updated_at: 2026-11-14
---

# wunder

wunder = model + harness + runtime

- **Harness**: the model's constraint layer — supplies tools and interfaces to the outside world, drives the model into action, and governs permissions and context.
- **Runtime**: browser, shell, files, Python, documents — prebuilt and ready once installed.

> **Read before use**
>
> - **Broad permissions**: wunder holds file read/write, command execution and network access on this machine. A wrong step can delete files, damage system configuration or break the runtime. Back up important data first, and confirm before anything that deletes, overwrites or touches system-level settings.
> - **Verify the output**: a model can produce wrong, outdated or harmful information. Check facts, figures, regulations and safety-related conclusions yourself before acting on them.

Its core function: the user issues instructions, and the agent completes long-running, complex tasks. The UI is radically minimal: solve the problem fast instead of showing off complexity — only what the current task needs, with deeper abilities expanded on demand.

## Where to start

Individual users start with Desktop: install it and you have a local AI workbench.

```text
Download Desktop → configure a model → describe a goal → get results
```

Teams use Cloud: deploy the Server once, then members open it in a browser. Use the CLI for hands-on and scripted work, Android on a phone, and the Admin for governance and debugging.

## What it can do

- **Files and code**: read, edit, run commands, refactor a project
- **Office automation**: organize documents, generate reports, process spreadsheets, take meeting notes
- **Long-running work**: attach a goal to a session so it keeps going when idle, see [Goal State System](/docs/en/concepts/goal-mode/)
- **Parallel subtasks**: spin up subagents when several threads must advance at once, then merge their results
- **Scheduled tasks**: periodic checks, recurring reminders, cross-channel messages
- **External services**: turn repeated flows into skills, or connect MCP tools
- **Device interlink**: once a local device joins Cloud, Cloud can send it commands with your approval

## The five forms

One engine (wunder-runtime) and one core (wunder-core) ship as five forms; the difference is only the access layer, since threads, tools, storage, realtime events and permission semantics are one body of code.

| Form | Purpose | Who it's for | How to reach it |
| --- | --- | --- | --- |
| [Desktop](/docs/en/forms/desktop-ui/) | Local AI workbench, installed on your own computer | Individual users (the default entry point) | Installer |
| [Cloud](/docs/en/forms/cloud-ui/) | Cloud AI workbench | Teams and multi-user setups | Browser |
| [CLI](/docs/en/forms/cli-ui/) | Manual local entry point | Command-line users / automation scripts | Terminal |
| [Android](/docs/en/forms/android-ui/) | The Desktop's phone form | Phone users | APK |
| [Admin](/docs/en/forms/admin-ui/) | Governance and debugging backend | Administrators | Browser |

Desktop, Android and the CLI are local forms that run on their own without the Server; Cloud ships with the Server; the Admin is the Server's own governance UI. Each form has four pages: **Interface**, **Deployment**, **Runtime** and **Usage**.

## Keep reading

- What the agent can call: [Tools](/docs/en/tools/)
- How the system is assembled: [Architecture](/docs/en/concepts/architecture/)
- Key mechanics: [Agent Loop](/docs/en/concepts/core-agent-loop/)
