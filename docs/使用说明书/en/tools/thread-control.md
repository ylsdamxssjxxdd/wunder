---
title: Thread Control
summary: Letting the agent move several things forward at once: create, switch, rename, archive, and restore task threads.
updated_at: 2026-10-13
---

The tool is `thread_control`, also known as `session_thread`.

A single agent can hold several independent task threads at once, and scheduling, locking, and live status are all isolated per thread. This tool manages the task threads themselves; the temporary units of work that a subagent dispatches and reclaims once done are a different matter, covered in [Subagent Control](/docs/en/tools/subagent-control/).

## When to use it

- The user raises a separate, independent matter that should be carried by a new thread rather than continued in the current one.
- You need to switch between threads, go back to a parent thread, or rename a thread.
- You need to archive or restore a thread, or look at its ancestors and child threads.

## How to use it

Each call specifies one action:

- `list`: list threads by scope.
- `info`: view a single thread's details, together with its parent thread and direct child threads.
- `create`: create a thread, inheriting the parent thread's agent and tool overrides by default.
- `switch`: switch to a given thread.
- `back`: switch back to the current thread's parent thread.
- `update_title`: change the title.
- `archive` / `restore`: archive / restore.

The available fields are `session_id` (the target thread, the current thread by default), `parent_session_id` (used only when creating, the current thread by default), `title` (for creating or renaming; when omitted on creation the thread is called "New session" by default), `scope`, `status`, and `limit`.

`scope` decides which threads are listed:

- `branch` (the default): the ancestor chain from the root to the current thread, plus the current thread and its direct child threads; each one is marked as an ancestor, the current thread, or a child.
- `children`: the direct child threads of the target thread.
- `roots`: the root threads that have no parent thread.
- `all`: all threads by status and agent scope.

Each thread in the list shows which one it is, its status, and when its latest message arrived. The "runtime status" there comes from live in-memory monitoring and is not the same measure as the persisted "active / archived".

## What it handles

- A newly created thread inherits the parent thread's agent and tool overrides.
- Archiving, restoring, and switching write the change to storage and refresh the session's modification time.
- When archiving is combined with switching away, it picks another active thread under the same agent to switch to.

## When something goes wrong

- A missing user identity, a target thread that does not exist, a rename with no title, or a "back to parent" when the current thread has no parent all raise clear errors.
- Viewing details, switching, archiving, and restoring all check agent ownership; an explicitly given agent that does not match the target thread is rejected as a permission error.
- A list holds at most 200 entries, and expanding child threads is bound by the same limit.
- The "switch away while archiving" only happens when switching is enabled and another active thread really exists under the same agent, and it picks the first other thread in the list, which may not be the intended one.
- A rename forces the status of a non-archived thread back to active.
- Switching threads is not the same as switching work: scheduling and locks are isolated per thread, and a thread you switch to that is running shows its status in live monitoring.
- It never deletes a thread; archiving is reversible and a restore brings it back.

## Form differences

Pupa lists this tool as a system-level hidden tool, in neither the default set nor the desktop selectable set; Beehive, Helm, and Hull expose it according to the administrator's selection.

## Related pages

- Temporarily dispatched units of work: [Subagent Control](/docs/en/tools/subagent-control/)
- Thread and round model: [Core Concepts · Sessions and Rounds](/docs/en/concepts/sessions-and-rounds/)
