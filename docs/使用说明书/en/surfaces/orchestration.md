---
title: Orchestration
summary: The orchestration page is a dedicated workbench for queen-worker-artifact collaboration. It lets one hive continue a long-running orchestration run, inspect round snapshots, and branch from earlier rounds.
read_when:
  - You want to use the orchestration page instead of a normal swarm run
  - You need to understand orchestration mode, timeline snapshots, artifacts, and branching
source_docs:
  - docs/设计文档/编排态系统设计.md
  - docs/方案/编排态系统落地方案.md
  - docs/API文档.md
updated_at: 2026-04-18
---

# Orchestration

Orchestration is not just another chat layout. It is the dedicated workbench used when a hive enters orchestration mode.

In this mode, the queen, workers, and round artifacts keep advancing inside one orchestration run, and the page lets you inspect the state round by round.

## When to Use It

Use orchestration when:

- You want the queen to coordinate workers across multiple user rounds
- You want artifacts to accumulate round by round and remain inspectable later
- You want to revisit an earlier round and continue from there as a new branch

If you only need a one-off parallel swarm task, the swarm page is lighter. If you need continuity, snapshots, and branching, use orchestration.

## How to Enter

A typical entry flow is:

1. Open the swarm area from the left navigation.
2. Select a hive in the middle column.
3. Open the orchestration page.
4. Click `New Orchestration` in the right-side controls.

Creating a new orchestration run forces the queen and all workers onto new orchestration main threads and switches the hive into that orchestration reality.

Important behavior:

- Only the initial `New Orchestration` creates fresh threads for everyone.
- Later user rounds continue on the same orchestration threads.

## Page Structure

The orchestration page uses the same interaction shell as the swarm canvas, but with different default spatial semantics.

### Canvas

The canvas keeps the same core behaviors as the swarm page:

- Draggable node cards
- Connectors
- Align / tidy action
- Fullscreen mode
- Matching side-panel and animation behavior

Its default layout meaning is:

- Queen card on the left
- Worker cards in the middle
- Artifact container cards on the right

This is not a rigid three-column table. Tidy alignment restores this default arrangement, but manual dragging is still allowed.

### Right Panel

The right panel is where you control the current run and send messages to the queen. Its main top actions are:

- New orchestration
- History
- Start / Close
- Situation

A small status lamp appears next to the hive title:

- Gray: inactive run
- Green: active and ready
- Blue: currently running

### Bottom Timeline

The timeline lives inside the canvas as a bottom dock rather than as a separate page block.

It represents the queen's user rounds, not generic chat threads. It supports:

- Collapse and expand
- Horizontal scrolling
- Visible branch structure
- Clicking a round to inspect messages and artifact snapshots from that time

## Typical Workflow

A common orchestration flow looks like this:

1. Create a new orchestration run.
2. Pre-fill situations for one or more rounds.
3. Send a message to the queen from the right panel.
4. The queen calls the swarm tool in orchestration mode.
5. Workers produce files inside the current round's artifact directories.
6. The queen aggregates the result and replies.
7. A formal user round is added to the timeline.

If you stop an in-progress round, that interrupted round does not become a formal orchestration round.

## Situation Presets

A situation is round-scoped background context.

Whenever you send a message to the queen, the system prepends the situation for that target round. When the queen dispatches workers during orchestration mode, the same round situation is also injected for workers.

At the user level, the important points are:

- Situations are stored per round
- Future rounds can be preset ahead of time
- You can jump to any round from the situation dialog
- You can import a `.txt` file to fill multiple rounds

The import format uses markers like `#1`, `#2`, and so on to separate round content.

## How Artifacts Are Displayed

Artifact nodes are container cards that display file slots.

When you select a round in the timeline, the page only shows files from that round's artifact snapshot. This makes it easy to answer "what exactly existed in round N?"

Key points:

- Artifact display switches by round snapshot
- It is driven by directory structure rather than extra business tags
- Each worker's files appear in its corresponding artifact container

## History, Replay, and Branching

### Viewing an Older Round

When you click an older round:

- The right panel shows only that round's message slice
- The artifact area shows only that round's files
- Sending is disabled

So you are inspecting a snapshot, not rewinding the live queen thread.

### Continuing from an Older Round

You can return to an older round and continue in two common ways:

- Modify that round's situation, then continue
- Leave the situation unchanged and continue anyway

As soon as you send from an older round, the system creates a new branch run instead of overwriting the old run.

That means:

- The old run stays intact
- The new branch continues from that round
- The timeline visualizes the branch relationship

### Deleting Later Content

Round nodes expose a context action for deleting everything after that round.

After deletion:

- The selected round stays
- Later rounds are removed
- Descendant branches grown from those later paths are also removed

## Relationship with the Chat Page

When a hive is in active orchestration mode, the corresponding orchestration threads are visibly marked on the normal chat page.

Those threads are protected. The chat page remains viewable, but it cannot be used to break the orchestration reality. In practice:

- You cannot send directly into the orchestration thread from normal chat
- You cannot replace it with a new main thread
- You cannot switch threads or change the main thread freely
- History controls can still be opened, but they cannot be used to disrupt the active orchestration state

To continue the orchestration, go back to the orchestration page.

## Start and Close

The right panel exposes `Start / Close` for the currently loaded run.

### Close

Closing a run:

- Does not leave the orchestration page
- Keeps that run as viewable history
- Switches the whole hive back to fresh normal main threads

### Start

If the currently loaded run is inactive, `Start` reactivates it as the current orchestration reality.

This does not create a new run. It re-enables the existing one.

## History List

The `History` action opens the hive's orchestration history.

From there you can:

- Inspect run timestamps
- Load an older run
- Delete a history item

When a historical run is loaded, the system tries to reuse the original queen thread. Missing worker threads can be recreated later if needed.

## Practical Advice

- Confirm worker roles before creating a new orchestration run.
- For long workflows, preset future situations first.
- If you want to compare alternate paths, continue from an older round and let the system create a branch.
- If you must interrupt a run, stop it from the orchestration panel instead of trying to operate through normal chat.

## Further Reading

- [User Frontend](/docs/en/surfaces/frontend/)
- [Swarm Collaboration](/docs/en/concepts/swarm/)
- [Stream Events](/docs/en/reference/stream-events/)
