---
title: Tools Overview
summary: wunder's tools are grouped by purpose: selection entry points by situation, the conventions for how results are shaped, and a few exceptions.
updated_at: 2026-10-13
---

wunder's tool system covers files, commands, the web, the desktop, collaboration, and system connections. Most built-in tools converge on one shared shell for both success and failure; a few stay outside it.

## Choose a tool by situation

### You only have keywords and want a web source

Web search has been removed; when you already have a URL, read the page directly with [Web Fetch](/docs/en/tools/web-fetch/).

### You only want to read the main content of a page

Start with [Web Fetch](/docs/en/tools/web-fetch/); do not jump straight to the browser.

### You need to click, type, or wait for dynamic rendering

Use [Browser](/docs/en/tools/browser/).

### You want to look at local code

The usual order is: list directories or search with [Workspace Files](/docs/en/tools/workspace-files/) first, then read the ranges you need.

### You want to change code

- Small, precise, context-matched edits: use [Apply Patch (merged into Text Edit)](/docs/en/tools/apply-patch/)
- One exact text replacement: use `文本编辑` in [Workspace Files](/docs/en/tools/workspace-files/)
- Complex replacement logic: run a script with [Execute Command](/docs/en/tools/exec/)
- Rewriting a whole file: use `write_file` in [Workspace Files](/docs/en/tools/workspace-files/)
- Compiling, testing, or running a script: use [Execute Command](/docs/en/tools/exec/)

### You want to collaborate

- Spinning up a temporary child run in the current session: use [Subagent Control](/docs/en/tools/subagent-control/)
- Managing the task thread or branch threads: use [Thread Control](/docs/en/tools/thread-control/)

## Tool groups

### Turn finishing and frontend coordination

- [Final Response (removed)](/docs/en/tools/final-response/)
- [Plan Panel / Question Panel / Sessions Yield](/docs/en/tools/panels-and-a2ui/)
- [Sessions Yield](/docs/en/tools/sleep/)

### Workspace and code

- [Workspace Files](/docs/en/tools/workspace-files/)
- [Execute Command](/docs/en/tools/exec/)
- [Apply Patch (merged into Text Edit)](/docs/en/tools/apply-patch/)
- [Skill Call](/docs/en/tools/skill-call/)
- [Read Image](/docs/en/tools/read-image/)
- [ptc (removed)](/docs/en/tools/ptc/)

### Web and desktop

- [Web Fetch](/docs/en/tools/web-fetch/)
- [Browser](/docs/en/tools/browser/)
- [Desktop Control](/docs/en/tools/desktop-control/)

### Threads and subagents

- [Thread Control](/docs/en/tools/thread-control/)
- [Subagent Control](/docs/en/tools/subagent-control/)

### System connections and memory

- [Self Status](/docs/en/tools/self-status/)
- [Memory Manager](/docs/en/tools/memory-manager/)
- [User World](/docs/en/tools/user-world/)
- [Channel Tool (removed)](/docs/en/tools/channel/)
- [A2A Tools (removed)](/docs/en/tools/a2a-tools/)
- [Schedule Task](/docs/en/tools/schedule-task/)

## How to read results

Most built-in tools return one shared shell on success: whether it succeeded, the action name, a state, a one-line summary, and a data area that carries the business result. Read the business result from the data area first. Some tools add one more line telling you what to do next.

Common states include: completed, dry run only, accepted, running, yielded, awaiting input, partially complete, timed out, and no change. A timeout does not always mean a permanent failure.

Failures use the same shared shell: an error line, a readable reason and fix hint, and data that helps with diagnosis.

A few exceptions do not use the shared shell:

- **Final Response** (removed): only a termination signal, returning a single answer to the user.
- **`a2ui`**: sends structured UI instructions to the frontend, with a thin wrapper in return.
- **Schedule Task**: returns a compact scheduling result.
- **Browser tool group**: results use a content-block structure (see [Browser](/docs/en/tools/browser/)).
- **Web Search** (removed): used to search the public web by keyword, with results in the data area's `results` field; to read a page now, use Web Fetch directly.
- **Web Fetch**: returns the fetched result object directly on success.
- **Text Edit** (formerly `apply_patch`): success uses the shared shell; in its patch form, failures carry stricter error semantics than ordinary file tools.

## See also

- How to read and write files and code → [Workspace Files](/docs/en/tools/workspace-files/)
- How subagents are spawned and reclaimed → [Subagent Control](/docs/en/tools/subagent-control/)
- What one specific tool returns → go to that tool's page
