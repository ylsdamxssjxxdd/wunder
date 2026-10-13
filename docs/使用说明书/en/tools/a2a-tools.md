---
title: A2A Tools
summary: A2A tools have been removed from wunder; kept only as a historical reference.
updated_at: 2026-10-13
---

> **Removed**: These tools have been removed from wunder and are not visible to the agent or users; kept only as a historical reference.

They once called an external A2A service and tracked task status, and came in three parts:

- `a2a@service-name`: submit a task, usually returning "accepted"
- `a2a_observe`: look at a task's current snapshot
- `a2a_wait`: wait for a while, until the task completes or times out

A timeout did not mean the task had failed: back then, a timeout was followed by more waiting or another look at the unfinished tasks before any conclusion was drawn.
