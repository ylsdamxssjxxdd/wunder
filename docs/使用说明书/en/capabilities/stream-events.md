---
title: Session Status Guide
summary: When the waiting, thinking, running tools, approval, failed, cancelled, stuck and connection statuses appear, what they mean, and how to handle them.
updated_at: 2026-10-16
---

During a conversation the interface shows statuses such as thinking, calling a tool, compressing context, or retrying after an error.

## Status overview

The states a conversation passes through from start to finish:

```
Waiting → Thinking / running → Done
             ↓
       Needs approval → waiting for your decision → continues
             ↓
       Error / cancelled → ends
```

## Basic statuses

| Status | Meaning | When it appears | What to do |
| --- | --- | --- | --- |
| Waiting | The request has been submitted and is queued for handling | The system is busy; several requests are running at once | Wait |
| Thinking | The agent is analyzing the request and preparing to answer or act | While the model understands the problem and works out a plan | Wait rather than resending the same message |
| Running tools | The agent is using tools to carry out the task: reading a file, searching the web, running code | Complex tasks need several tools working together | Watch the progress in the progress area |
| Done | The agent has finished and given its final answer | Request handling has ended | Follow up, review the content or files it produced, or start a new task |

While tools run, the interface shows prompts such as "Reading file…", "Searching…", and "Running…".

## Approval statuses

| Status | Meaning | What to do |
| --- | --- | --- |
| Approval needed | The agent wants an action that needs confirmation: changing an important file or running a sensitive command; or an approval mode is configured | The interface shows the approval request and a description of the action; click Approve to continue, or Reject to stop it |
| Approval resolved | A decision has been made on the approval request | Approved: the agent continues with the action. Rejected: it drops the action and may look for another approach |

Approval is triggered because the action may be risky or affect important files; it is a safety mechanism. To see fewer approvals, talk to an administrator about adjusting the approval mode.

## Abnormal statuses

| Status | Meaning | Causes | What to do |
| --- | --- | --- | --- |
| Failed | The agent ran into a problem and could not finish the task | A file is missing or unreadable; a network request failed; a tool errored out; the AI model service had a problem | Read the error for the cause, adjust the request as it suggests, such as giving the right file path, then retry |
| Cancelled | The conversation was stopped, by you or by the system | You clicked Stop; the system timed out and cancelled; some other deliberate stop | Start the conversation again, or review the record of what ran before the cancellation |
| Stuck session | It stays in one state for a long time with no progress | The network connection dropped; the backend service had a problem; task handling timed out | Wait a few minutes first; if it is genuinely stuck, click Stop; use the reset work state option in settings to clear the abnormal state; if it still does not recover, contact an administrator |

## Connection status

| Status | Meaning | Symptoms and handling |
| --- | --- | --- |
| Connected | The network connection to the backend service is fine | — |
| Disconnected | The network connection to the backend service has dropped | You cannot send new messages, the status display looks wrong, and a "connection lost" notice appears; check your network, refresh the page or restart the app, and wait for the automatic reconnect |

## Where status indicators appear

| Location | What it shows |
| --- | --- |
| Above the conversation | The overall status of the current session |
| Beside a message bubble | The processing status of that message |
| In the progress area | Detailed progress of tool execution |
| Lower or right part of the chat | The run trace and tool call record: which tools ran, which files were read, what actions were performed |

## Diagnosis

**Slow responses**: a complex task that needs several steps, a slow AI model service, or high system load all lengthen the response time. Wait rather than resending the same message.

**Status display looks wrong**: wait first to see whether it recovers on its own; if not, use the reset work state option; then refresh the page or restart the app. If it still persists, follow [Troubleshooting](/docs/en/help/troubleshooting/).
