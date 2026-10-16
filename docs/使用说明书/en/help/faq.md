---
title: FAQ
summary: Common conclusions on registration, usage, requirement scope, memory loading, the thread button, subagents, passwords, and tool visibility.
updated_at: 2026-10-16
---

## Registration and Sign-in

No registration is required. You can start a conversation under any name, and the system creates the matching identity automatically.

## Cost and Usage

The token count is not the cost. It only reflects the context length of the current conversation; the actual cost depends on your model provider's billing rules.

## Scope of Requirement Settings

Requirements set at the start of a conversation are remembered and locked in. Later messages follow them, so you don't need to repeat them.

## When Memory Loads

Long-term memory loads once, at the start of a conversation. It is not re-read on every turn.

## When the New Thread Button Is Disabled

The button is briefly disabled while a thread is being created or switched. Threads already running in the background do not block a new thread, and an existing unused empty thread is reused first.

## Subagents

A subagent is a temporary work unit the agent creates during a task, used to split off work that can proceed in parallel and get it done; once the result is returned to the main thread, the subagent is reclaimed. It is not a separate agent — on the user side there is always exactly one agent instance.

## Pupa and Hull Dependency

Deploying the Hull is not required. The Pupa can run locally on its own; deploy the Hull when you need multi-person collaboration and unified management.

## Changing Your Username and Password

Change them under "My Profile → Edit". Changing your password requires entering your current password.

## Password Recovery

On the login page, click "Reset Password". You need your username, email, and a new password — no old password required.

## Cleanup of Uploaded Files

Files in the temporary directory may be cleaned up. Keep important files in the workspace's persistent directory.

## Tool Visibility

Possible causes: the tool is not enabled, the MCP/A2A service is not running, or the current agent does not have that tool mounted.

## Difference Between web_fetch and the Browser Tool

- web_fetch: grabs the main text content of a webpage
- The browser tool: operates a webpage like a real person (clicking, scrolling, filling forms)

## Where to Look When Something Goes Wrong

Symptom-level troubleshooting lives in [Troubleshooting](/docs/en/help/troubleshooting/).
