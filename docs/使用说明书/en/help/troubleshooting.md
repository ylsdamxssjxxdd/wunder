---
title: Troubleshooting
summary: Follow the "entry -> auth -> config -> dependencies -> realtime channel" sequence to quickly locate most Wunder issues.
read_when:
  - Wunder won't start or behaves abnormally
  - You've confirmed it's not a simple usage issue
source_docs:
  - docs/API文档.md
  - frontend/src/views/LoginView.vue
  - frontend/src/views/MessengerView.vue
  - frontend/src/components/messenger/DesktopRuntimeSettingsPanel.vue
updated_at: 2026-04-10
---

# Troubleshooting

It's recommended to troubleshoot by link order, not by exhaustively scanning logs first.

## 60-Second Health Check

1. Are core endpoints reachable: `/wunder`, `/wunder/chat/ws`
2. Is auth matching: API Key / User Token / External Auth
3. Are dependencies ready: database, sandbox, MCP

## Symptom -> Check Path

### 1. API Returns 401 / 403 Directly

Check first:

- Is a user token mistakenly used for admin endpoints
- Is an API Key mistakenly used for user endpoints
- Are `/a2a`, `/wunder/mcp` carrying an API Key
- Is `external_auth_key` configured for external link scenarios

### 2. Configuration Changed but Not Taking Effect

Check first:

- Is the actual loaded file `config/wunder.yaml` or a sample file
- Is the current instance actually reading `config/wunder.yaml` or local runtime `WUNDER_TEMP/config/wunder.yaml`
- Are you modifying server config, extra_mcp config, or frontend config

### 3. Service Starts Successfully but Capabilities Unavailable

Check dependencies first:

- Is PostgreSQL / SQLite connectable
- Is sandbox reachable
- Is extra_mcp started
- Are external MCP/A2A targets online

### 4. Real-time State Not Updating, Can't See Intermediate Process

Check first:

1. Did `/wunder/chat/ws` connection succeed
2. Has it fallen back to SSE
3. Are `session_id`, `after_event_id` correct

### 5. Tools Not Appearing or Can't Be Invoked

Check first:

- Is the tool enabled
- Is MCP / A2A service `enabled`
- Is the target tool mounted to current session or agent
- Is it stuck in approval state but frontend didn't send back `approval`

### 6. Attachment Stuck Processing, or Can't Send After Upload

Check first:

- Is the uploaded type currently supported: images, audio, video, common text or Office documents
- Is document conversion pipeline working: `POST /wunder/chat/attachments/convert`
- Is media processing pipeline working: `POST /wunder/chat/attachments/media/process`
- Are `temp_dir` and user private workspace writable
- If video re-frame extraction failed, check if source file still exists, if `source_public_path` is still valid

Additional notes:

- Chat input area blocks sending before attachment processing completes; this is normal protection
- Long videos are automatically limited in total frames, so "requested FPS" and "actual FPS" may differ

### 7. Login Page Password Reset Failed

Check first:

- Is the username correct
- Does the email match this account
- Are the two new password entries consistent

Additional notes:

- Login page password reset only recognizes "username + email + new password"
- It doesn't require prior login or old password

### 8. Edit Profile or Change Password Save Failed

Check first:

- Is username empty
- If changing password, did you enter current password
- Are new password and confirm new password consistent
- Is new password the same as current password

Additional notes:

- When only changing username or email, password fields can be left empty
- Password change after login always validates current password

### 9. New Thread Button Is Grayed Out

First determine if the current session is still running.

This is frontend protection, not a bug. When the current agent is executing, the chat page disables `New Thread` to prevent main thread state confusion.

Resolution:

- Wait for current execution to complete
- Or stop current session first, then create new thread

### 10. Swarm Page State Looks Incorrect

Check first:

- Is the swarm realtime channel still online
- Is the current mission actually in terminal state, but frontend hasn't received latest event
- Does the page return to normal after refresh

Additional notes:

- Middle column swarm entries will pulse-highlight as long as there are running missions
- Canvas workflow area currently prioritizes showing tool trajectories; it shouldn't long-term show only status/summary

### 11. Session or Swarm State Completely Messed Up

Try "System Settings -> Reset Work State" first.

It will:

- Abort running sessions
- Clear queued tasks
- Terminate swarm execution
- Rebuild default agent and each user agent's main thread
- Clean up working state directories

Additional notes:

- It clears working state, not long-term assets
- Long-term content like `skills`, `knowledge`, `global` is preserved

### 12. Microphone or Screenshot Button Unavailable

Check first:

- Is current runtime environment supporting the corresponding capability
- Is microphone permission denied by system or browser
- Does browser support `getUserMedia` / `MediaRecorder` / `AudioContext`
- Does desktop bridge expose recording or screenshot capabilities

### 13. Help Manual Blank or Won't Open

Check first:

- Is `/docs/` static site built and accessible
- Does reverse proxy correctly expose `/docs/` to the frontend domain
- If using remote API mode, does the current origin actually provide `/docs/`

## Still Can't Locate

Return to these pages to narrow down:

- [Desktop Local Mode](/docs/en/ops/desktop-local-mode/)
- [Authentication & Security](/docs/en/ops/auth-and-security/)
- [Configuration Reference](/docs/en/reference/config/)
- [Stream Events Reference](/docs/en/reference/stream-events/)

## Suggested Information When Submitting Issues

- Runtime mode: `desktop / server / cli`
- Failed endpoint and timestamp
- Key log snippets
- Whether WS, SSE, Swarm, Sub-Agent, MCP, A2A are involved