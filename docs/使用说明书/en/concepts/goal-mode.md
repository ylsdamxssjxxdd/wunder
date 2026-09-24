---
title: Goal Mode
summary: Set a goal so the agent keeps working until the task is complete or the budget runs out. Ideal for complex tasks that need multiple rounds.
---

# Goal Mode

Goal mode is a task-driven mode. Once you set a goal, the agent keeps working until the goal is complete, automatically handling waiting, retries, budget checks, and other edge cases along the way.

## When to Use

Goal mode fits:

- **Complex tasks**: work that needs multiple rounds, such as "develop a complete user authentication module"
- **Long-running tasks**: the agent must keep working and cannot finish in one reply
- **Clear completion criteria**: the task has a well-defined "done"
- **Budget control**: you want to cap the tokens the agent can spend

**Poor fit for goal mode:**

- Simple Q&A (one reply solves it)
- Open-ended exploration (no clear completion criteria)
- Tasks needing frequent human intervention

## Starting Goal Mode

### Option 1: Use a Command

Type in the chat input:

```
/goal Develop the user authentication module
```

### Option 2: Use the Goal Editor

1. Click the target icon above the chat panel (bullseye icon)
2. Enter the goal description in the dialog
3. Optional: set a token budget
4. Click "Start"

### Option 3: Let the Agent Create It

The agent can create a goal through the `goal` tool (action=create). This usually happens when the agent decides the task needs sustained execution.

## Goal Status

| Status | Meaning | Color |
|--------|---------|-------|
| Active | Executing | Green |
| Paused | Paused by the user | Yellow |
| Budget limited | Token budget exhausted | Red |
| Complete | Goal finished | Gray |

### Active

While the goal is active:

- The agent keeps executing
- Each round automatically continues to the next
- The companion shows the "running" animation
- The message list shows a goal divider

### Paused

After pausing:

- The agent stops executing
- Resume with `/goal resume`
- Or resume from the goal editor

### Budget Limited

When a token budget was set and is exhausted:

- The goal automatically enters budget-limited status
- The agent stops executing
- User intervention is required (raise the budget or clear the goal)

### Complete

After the agent marks completion via the `goal` tool (action=update):

- The goal is marked complete
- Automatic execution stops
- The completion time is shown

## Token Budget

### Setting a Budget

Set a token budget in the goal editor:

```
Goal: Develop the user authentication module
Budget: 100000 tokens
```

### Budget Consumption

After each round, the system accumulates:

- Input tokens
- Output tokens
- Tool call tokens

When total consumption reaches the budget, the goal automatically enters "budget limited" status.

### Budget Suggestions

Pick a reasonable budget based on complexity:

| Task type | Suggested budget |
|-----------|------------------|
| Simple feature | 10,000 - 30,000 |
| Medium feature | 30,000 - 100,000 |
| Complex feature | 100,000 - 500,000 |
| Large project | 500,000+ |

## Execution Flow

### Automatic Continuation

While the goal is active, execution continues automatically:

1. A round finishes
2. Goal status and budget are checked
3. A 1-second cooldown is observed
4. The continuation prompt is injected
5. The next round starts

### Waiting for User Input

If the agent needs your input to continue:

- The goal does not auto-continue
- It resumes after you reply

### Completion

When the agent judges the goal complete, it marks completion via the `goal` tool (action=update), and automatic execution stops.

## Leaving Goal Mode

### Normal Exit

The agent exits automatically after completing the task.

### Manual Exit

Click the stop button on the chat panel, or use:

```
/stop
```

or

```
/cancel
```

### Notes

- Leaving goal mode clears the current goal
- Actions already taken are not rolled back
- You can set a new goal afterwards

## Frontend Display

### Goal Divider

The start of a goal shows a divider in the message list:

```
────────── Goal started: Develop the user authentication module ──────────
```

### Status Badge

The top of the chat panel shows the current goal status:

- Active: 🟢 goal in progress
- Paused: 🟡 goal paused
- Budget limited: 🔴 budget exhausted
- Complete: ⚪ goal completed

### Goal Lock

While goal mode is active:

- You cannot switch to another session
- You cannot switch agents
- The companion shows the running state

## Command Reference

| Command | Purpose |
|---------|---------|
| `/goal` | Open the goal editor |
| `/goal <description>` | Set a goal directly |
| `/goal --tokens 100000 <description>` | Set a goal with a budget |
| `/goal pause` | Pause the goal (command only) |
| `/goal resume` | Resume the goal (command only) |
| `/goal clear` | Clear the goal (command only) |
| `/stop` or `/cancel` | Leave goal mode |

## Best Practices

### Clear Goal Descriptions

A good goal description:

```
Develop the user authentication module, including:
1. Registration (email verification)
2. Login (with remember-me)
3. Password reset (email link)
4. Session management (auto expiry)
```

A poor goal description:

```
Write some code
```

### Reasonable Budgets

- Not too low: the task may not finish
- Not too high: budget control loses meaning
- Adjust based on complexity

### Intervene in Time

- Watch the agent's progress
- Correct course early when it drifts
- Pause the goal to adjust direction when needed

## FAQ

### The goal never completes

Possible causes:

- The goal description is not specific enough
- The agent judges there is still work to do
- A blocking problem was hit

Fixes:

- Review the agent's output
- Provide more context
- Exit manually and set a more specific goal

### Budget exhausted but task unfinished

Fixes:

- Set the goal again with a higher budget
- Narrow the task scope
- Complete it in stages

### Goal execution interrupted

Possible causes:

- Network issues
- Service restart
- System error

Fixes:

- Check the network connection
- Refresh the page
- Set the goal again

## Relationship to Other Features

| Feature | Relationship |
|---------|--------------|
| Companion system | The companion shows the running state during execution |
| Context compression | The goal description is preserved as important context |
| Swarm collaboration | A goal can trigger sub-agent collaboration |
| Tool system | The agent operates goals through the `goal` tool |

## Next Steps

- [Companion System](/docs/en/concepts/companion/) — give agents an animated companion
- [Agent Loop](/docs/en/concepts/core-agent-loop/) — how agents execute tasks
- [Context Compression](/docs/en/concepts/core-context-compression/) — how long conversations stay efficient
