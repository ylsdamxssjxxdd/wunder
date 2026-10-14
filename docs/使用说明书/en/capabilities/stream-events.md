---
title: Session Status Guide
summary: What the various statuses in a chat mean, what the agent is doing, and what to do when something goes wrong.
updated_at: 2026-10-13
---

During a conversation the interface shows statuses such as thinking, calling a tool, compressing context, or retrying after an error.

## Status overview

A conversation usually passes through these states from start to finish:

```
Waiting → Thinking / running → Done
             ↓
       Needs approval → waiting for your decision → continues
             ↓
       Error / cancelled → ends
```

## Basic statuses

### Waiting

**Meaning**: the request has been submitted and is queued for handling.

**When it appears**:

- When the system is busy
- When several requests are running at once

**What to do**: wait; it usually starts soon.

### Thinking

**Meaning**: the agent is analyzing your request and preparing to answer or act.

Understanding the problem and working out a plan takes time, so this is a normal state. Wait a moment rather than resending the same message.

### Running tools

**Meaning**: the agent is using tools to carry out the task, such as reading a file, searching the web, or running code.

**Messages you may see**:

- "Reading file…"
- "Searching…"
- "Running…"

Complex tasks often need several tools working together, and the interface shows the progress.

### Done

**Meaning**: the agent has finished and given its final answer.

**What you can do next**:

- Follow up with another question
- Review the content or files it produced
- Start a new task

## Approval statuses

### Approval needed

**Meaning**: the agent wants to perform an action that needs your confirmation, such as changing an important file or running a risky command.

**When it appears**:

- The agent wants to change an important file
- The agent wants to run a sensitive command
- An approval mode is configured

**What you see**:

- An approval popup or prompt
- A description of the action the agent wants to take

**What to do**:

- Read the description carefully
- Decide whether to approve
- Click Approve to continue, or Reject to stop it

### Approval resolved

**Meaning**: you have made a decision on the approval request.

**Two outcomes**:

- **Approved**: the agent continues with the action
- **Rejected**: the agent drops it and may look for another approach

## Abnormal statuses

### Failed

**Meaning**: the agent ran into a problem and could not finish the task.

**Possible causes**:

- A file is missing or unreadable
- A network request failed
- A tool errored out
- The AI model service had a problem

**What to do**:

- Read the error for the cause
- Adjust the request as it suggests, such as giving the right file path
- Retry

### Cancelled

**Meaning**: the conversation was stopped, either by you or by the system.

**When it appears**:

- You clicked Stop
- The system timed out and cancelled
- Some other deliberate stop

**What you can do next**:

- Start the conversation again
- Review the record of what ran before the cancellation

### Stuck session

**Symptoms**: it stays in one state for a long time with no progress.

**Possible causes**:

- The network connection dropped
- The backend service had a problem
- Task handling timed out

**What to do**:

1. Wait a few minutes first (sometimes processing just takes a while)
2. If it is genuinely stuck, click Stop
3. Use the reset work state option in settings to clear an abnormal state
4. If that still does not help, contact an administrator or see the troubleshooting guide

## Connection status

### Connected

**Meaning**: the network connection to the backend service is fine.

### Disconnected

**Meaning**: the network connection to the backend service has dropped.

**Symptoms**:

- You cannot send new messages
- The status display looks wrong
- A "connection lost" notice appears

**What to do**:

- Check your network
- Refresh the page or restart the app
- Wait for it to reconnect automatically

## Where status indicators appear

In the chat interface, status appears in these places:

- **Above the conversation**: the overall status of the current session
- **Beside a message bubble**: the processing status of that message
- **In the progress area**: detailed progress of tool execution

## Common situations

### Why responses slow down

Possible reasons:

- The task is complex and needs several steps
- The AI model service is responding slowly
- The system is under high load

Wait rather than resending the same message.

### Why the agent waits for approval

The action may be risky or affect important files. This is a safety mechanism that makes sure sensitive actions are confirmed. To see fewer approvals, talk to an administrator about adjusting the approval mode.

### When the status display looks wrong

1. Wait first and see whether it recovers on its own
2. Use the reset work state option
3. Refresh the page or restart the app
4. Check the troubleshooting guide

### Seeing what the agent did recently

The lower or right part of the chat usually holds a run trace or a record of tool calls, showing which tools the agent used, which files it read, and what actions it performed.

## Further reading

- [Troubleshooting](/docs/en/help/troubleshooting/)
- [Core Agent Loop](/docs/en/concepts/core-agent-loop/)
- [Tool System](/docs/en/concepts/tools/)
