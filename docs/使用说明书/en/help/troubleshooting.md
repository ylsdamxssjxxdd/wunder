---
title: Troubleshooting
summary: Find the cause by symptom. Quick diagnosis for most issues.
read_when:
  - wunder won't start or behaves abnormally
  - You've confirmed it's not a simple usage question
source_docs:
  - docs/API文档.md
updated_at: 2026-04-10
---

# Troubleshooting

Find the cause by symptom. Don't jump straight to logs.

## 60-Second Health Check

1. Is the service running? Can you open the page?
2. Is login working? Are your credentials correct?
3. Is the model configured correctly? Does the API Key work?

## Find by Symptom

### Login Fails or Access Denied

Check:
- Are your username and password correct?
- Are you using the right entry point (admin vs. user)?
- Is your account disabled?

### Configuration Changes Not Taking Effect

Check:
- Did you edit the correct config file?
- Does the change require a service restart?
- Is the current instance actually loading your config?

### Service Starts But Features Don't Work

Check:
- Is the database connected properly?
- Are external tool services (MCP, A2A) online?
- Is the sandbox service reachable?

### Real-time Updates Not Working, Can't See Intermediate Steps

Check:
- Is your network connection stable?
- Is WebSocket being blocked by a firewall or proxy?
- Does it recover after refreshing the page?

### Tools Not Appearing or Can't Be Called

Check:
- Is the tool enabled?
- Are MCP / A2A services running?
- Does the current agent have that tool mounted?
- Is it waiting for approval but you didn't see the prompt?

### Attachment Uploaded But Can't Send

Check:
- Is the file type supported (images, audio, video, common documents)?
- Is the file still being processed (send button is disabled until processing completes)?
- Does the workspace have write permission?

### Forgot Password

Click "Reset Password" on the login page. You only need:
- Username
- Email
- New password

No old password required, and no need to be logged in first.

### Password Change Fails

Check:
- Did you enter your current password (required when changing password while logged in)?
- Do the new password and confirmation match?
- Is the new password the same as the current one?

### New Thread Button Is Grayed Out

This is not a bug. The system disables new thread creation while the current agent is running. Wait for it to finish or stop the current session.

### Swarm Page State Looks Wrong

Check:
- Does it recover after refreshing?
- Has the swarm task already finished but the page hasn't updated?
- Is the real-time channel disconnected?

### Session or Swarm State Completely Messed Up

Use "System Settings → Reset Workspace State".

It will:
- Stop running sessions and tasks
- Terminate swarm operations
- Rebuild clean threads

It will NOT delete your skills, knowledge bases, or other long-term assets.

### Microphone or Screenshot Button Not Working

Check:
- Are you in an environment that supports these features?
- Has the browser granted microphone/screen permissions?
- Is the desktop bridge working properly?

### Help Manual Blank or Won't Open

Check:
- Has the docs site been built?
- Is the reverse proxy correctly configured for the `/docs/` path?

## Still Not Resolved?

Continue with:

- [Desktop Local Mode](/docs/en/ops/desktop-local-mode/)
- [Authentication & Security](/docs/en/ops/auth-and-security/)
- [Configuration Reference](/docs/en/reference/config/)

## When Reporting an Issue, Include

- Runtime mode: desktop / server / cli
- Steps to reproduce the problem
- Key error messages
- Whether it involves swarms, MCP, A2A, etc.
