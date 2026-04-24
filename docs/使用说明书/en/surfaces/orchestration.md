---
title: Orchestration
summary: The orchestration page is a dedicated workbench for swarm tasks that need continuous advancement. Advance by rounds, view history, generate branches.
---

# Orchestration

Orchestration is the dedicated workbench for swarms in "continuous advancement mode".

The regular swarm page is for one-time collaboration. The orchestration page is for scenarios that need **multi-round advancement, round-by-round records, and branching from old rounds**.

## When to Use Orchestration

- The queen bee needs to continuously coordinate multiple worker bees, not just do a one-time task
- You want to accumulate artifacts round by round for later review
- You want to continue from old rounds to form new branches, not overwrite old results

If you just need temporary parallel collaboration, the regular swarm page is lighter.

## How to Enter

1. Open the swarm area in the left column
2. Select a swarm in the middle column
3. Click "Orchestration" to enter the orchestration page
4. Click "New Orchestration" at the top of the right sidebar

When you create a new orchestration, the system establishes new threads for the queen bee and all worker bees, and switches the swarm to this orchestration state.

Note: Only the first "New Orchestration" creates new threads. Subsequent rounds reuse the same set of threads to maintain context continuity.

## Page Layout

### Canvas

- Left: Queen bee card
- Center: Worker bee cards
- Right: Artifact container card

Nodes are draggable, with connection lines and auto-arrangement support.

### Right Sidebar

Controls the current orchestration run:

- **New Orchestration**: Create a new orchestration run
- **History**: View past runs
- **Start / Stop**: Control whether the current run is active
- **Situation Settings**: Preset background info for each round

Status light next to the swarm title:
- Gray = Not active
- Green = Active, can continue advancing
- Blue = Currently executing

### Bottom Timeline

The timeline shows the queen bee's user turns, supporting:
- Collapse and expand
- Horizontal scrolling
- Branch visualization
- Click a round to view its messages and artifacts

## Basic Workflow

1. Create new orchestration
2. Preset background info for each round in the "Situation" window
3. Send a message to the queen bee
4. Queen bee calls the swarm tool, activating worker bees
5. Worker bees produce files in the current round's artifact directory
6. Queen bee summarizes results and replies
7. Timeline adds a new round, ready for the next

If a round is stopped by you, it doesn't count as an official round.

## Situation Settings

Situations are background info for each round, automatically appended to the message context.

- Situations are saved per round
- You can pre-set future rounds
- Supports importing `.txt` files for batch writing (use `#1`, `#2` to separate rounds)

## Artifact Viewing

When you select a round in the timeline, the page only shows that round's artifact files — directly answering "what did round N produce?"

## History, Review, and Branching

### Reviewing Old Rounds

Click an old round in the timeline:
- Right sidebar shows that round's messages
- Artifact area shows that round's files
- Send input is disabled (view only, no modifications)

### Continuing from Old Rounds

When you continue sending from an old round, the system automatically creates a new branch run instead of overwriting the original.

- Original run is preserved unchanged
- New branch continues from that round
- Timeline shows the trunk and branch relationship

### Deleting Subsequent Content

You can delete "everything after this round" on a timeline node. The current round is preserved; subsequent rounds and branches are removed together.

## Orchestration Mode and Chat Page Relationship

When a swarm is in active orchestration mode, orchestration threads in the chat page have clear indicators, and:

- You can't send messages directly to orchestration threads
- You can't create new threads that override the orchestration main thread
- Historical threads can be viewed but can't disrupt the orchestration state

To continue orchestration, go back to the orchestration page.

## Start and Stop

### Stop

Stopping doesn't leave the orchestration page. The current run is preserved as viewable history, and the swarm switches back to its regular main thread.

### Start

When you open an inactive historical run, you can click "Start" to reactivate it. This is not creating a new run — it's re-enabling the current orchestration.

## Usage Tips

- Before creating an orchestration, confirm swarm members and roles are configured
- For long-running tasks, pre-set several rounds of situations before starting
- When you want to compare two approaches, continue from an old round to generate a branch — don't overwrite old rounds
- If you need to interrupt during orchestration, use the stop button in the right sidebar — don't switch to the chat page to operate

## Further Reading

- [User Interface](/docs/en/surfaces/frontend/)
- [Swarm Collaboration](/docs/en/concepts/swarm/)
- [Troubleshooting](/docs/en/help/troubleshooting/)
