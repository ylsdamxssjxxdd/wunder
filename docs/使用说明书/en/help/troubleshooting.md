---
title: Troubleshooting
summary: Cause and check points by symptom.
updated_at: 2026-10-13
---

Find the cause by symptom.

## Health check

1. Make sure the service is running and the page opens.
2. Make sure you can sign in and your account and password are correct.
3. Make sure the model is configured correctly and the API key works.

## Find by Symptom

### Login Fails or Access Denied

Check:
- Whether your username and password are correct
- Whether you used the wrong entry point for the Bridge and the Beehive
- Whether your account is disabled

### Configuration Changes Not Taking Effect

Check:
- Whether you edited the right configuration file
- Whether the change needs a service restart
- Whether the running instance actually loaded the changed configuration

### Service Starts but Features Are Unavailable

Check:
- Whether the database is connected properly
- Whether the external tool services (MCP, A2A) are online
- Whether the sandbox service is reachable

### Real-time Status Not Updating, No Intermediate Steps Visible

Check:
- Whether your network connection is stable
- Whether WebSocket is being blocked by a firewall or proxy
- Whether it recovers after refreshing the page

### Tools Don't Appear or Can't Be Called

Check:
- Whether the tool is enabled
- Whether the MCP / A2A service is running
- Whether the current agent has that tool mounted
- Whether it is waiting for approval and you missed the prompt

### Attachment Uploaded but Can't Be Sent

Check:
- Whether the file type is supported (images, audio, video, common documents)
- Whether the file is still being processed (the send button stays disabled until processing finishes)
- Whether the workspace has write permission

### Password Recovery

Click "Reset Password" on the login page. You only need:
- Username
- Email
- New password

No old password is required, and you don't need to sign in first.

### Password Change Won't Save

Check:
- Whether you entered your current password (changing a password while signed in always verifies the old one)
- Whether the new password and the confirmation match
- Whether the new password is the same as the current one

### New Thread Button Is Grayed Out

The button is briefly disabled while a thread is being created or switched. Threads running in the background can still start or switch, and an existing unused empty thread is reused first.

### Session State Completely Messed Up

Use "System Settings → Reset Working State".

It will:
- Stop running sessions and tasks
- Rebuild clean threads

It will not delete skills, knowledge bases, or other long-term assets.

### Microphone or Screenshot Button Unavailable

Check:
- Whether you are in an environment that supports these features
- Whether the browser has been granted microphone/screen permission
- Whether the desktop bridge is working properly

### Help Manual Blank or Won't Open

Check:
- Whether the docs site has been built
- Whether the reverse proxy is configured correctly for the `/docs/` path

### Capturing a Performance Report for Chat Stutter

Open "More → Settings → Performance Capture" and follow three steps:

1. Click "Start", then return to chat and reproduce the stutter while replying, scrolling, or switching pages.
2. Go back to settings and click "Download JSON"; the browser downloads the report directly, ready to attach to your issue report.
3. Click "Stop" to end capture. You can still download afterward; starting again creates a new report.

No console or reload is needed to start. If you reload the page yourself, this tab keeps capturing and retains the statistics from before the reload, for up to 30 minutes. Browser metrics that aren't supported are marked unavailable.

The report includes frame gaps, main-thread long tasks, slower input events, chat rendering and loading timings, periodically sampled DOM/memory peaks, and the browser and app versions. It excludes message bodies, call arguments, accounts, session identifiers, and file paths. Downloading doesn't end capture and doesn't upload any file.

When you submit, add what you did and when the stutter happened. Total model reply time and Worker wait time are not the same as main-thread stutter, and the report lists them separately.

## Still Unresolved

Continue with:

- [Configuration Reference](/docs/en/capabilities/config/)

## What to Include When Reporting an Issue

- Runtime form: desktop / server / cli
- Steps that reproduce the problem
- Key error messages
- Whether it involves subagents, MCP, A2A, etc.
