---
title: Integration Overview
summary: Each integration need has its own entry: one-off execution, chat sessions, workspace, temporary conversion, external channels, passwordless embedding, A2A, and MCP.
updated_at: 2026-10-13
---

Pick the interface domain first, then the specific interfaces inside it.

## Integration Domains

| What you want to do | Which entry to use | Why |
| --- | --- | --- |
| One-off execution, lightweight integration | [wunder API](/docs/en/integration/wunder-api/) | One step, a single request returns a result |
| A chat product with session management | [Chat Sessions](/docs/en/integration/chat-sessions/) + [Chat WebSocket](/docs/en/integration/chat-ws/) | Full session lifecycle + real-time push |
| Managing files and artifacts | [Workspace API](/docs/en/integration/workspace-api/) | Unified upload, download, and directory management |
| Document conversion, temporary downloads | [Temp Directory](/docs/en/integration/temp-dir/) | doc/pdf to Markdown, temporary file sharing |
| Connecting external channels | [Channel Webhook](/docs/en/integration/channel-webhook/) | Unified entry for Feishu, WeChat, QQ, XMPP |
| Connecting WeChat, telling old from new | [WeChat iLink Channel](/docs/en/integration/weixin-channel/) | Tell new from legacy WeChat, QR-code onboarding |
| Embedding into an existing system, passwordless | [External Login](/docs/en/integration/external-login/) | Passwordless embedding |
| Agent-to-agent calls | [A2A Interface](/docs/en/integration/a2a/) | A standard protocol for cross-system collaboration |
| Mounting third-party tools | [MCP Endpoint](/docs/en/integration/mcp-endpoint/) | Mounting external MCP services |

## Integration Surface Overview

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/integration/wunder-api/">
    <strong>wunder API</strong>
    <span>Shortest-path execution entry.</span>
  </a>
  <a class="docs-card" href="/docs/en/integration/chat-sessions/">
    <strong>Chat Sessions</strong>
    <span>Session lifecycle and messaging interface.</span>
  </a>
  <a class="docs-card" href="/docs/en/integration/chat-ws/">
    <strong>Chat WebSocket</strong>
    <span>Main channel for real-time sessions.</span>
  </a>
  <a class="docs-card" href="/docs/en/integration/workspace-api/">
    <strong>Workspace API</strong>
    <span>Files, upload, write, routing.</span>
  </a>
  <a class="docs-card" href="/docs/en/integration/temp-dir/">
    <strong>Temp Directory</strong>
    <span>Document conversion and download chain.</span>
  </a>
  <a class="docs-card" href="/docs/en/integration/user-world/">
    <strong>User World API</strong>
    <span>In-system user messaging pathway.</span>
  </a>
  <a class="docs-card" href="/docs/en/integration/channel-webhook/">
    <strong>Channel Webhook</strong>
    <span>Unified inbound entry for external channels.</span>
  </a>
  <a class="docs-card" href="/docs/en/integration/weixin-channel/">
    <strong>WeChat iLink Channel</strong>
    <span>New WeChat onboarding, QR login, and troubleshooting.</span>
  </a>
  <a class="docs-card" href="/docs/en/integration/external-login/">
    <strong>External Login</strong>
    <span>Passwordless linking and embedding.</span>
  </a>
  <a class="docs-card" href="/docs/en/integration/a2a/">
    <strong>A2A Interface</strong>
    <span>Agent system interconnection protocol.</span>
  </a>
  <a class="docs-card" href="/docs/en/integration/mcp-endpoint/">
    <strong>MCP Endpoint</strong>
    <span>MCP service mounting and discovery.</span>
  </a>
</div>

## The Three Most Common Entry Points

### One-Off Execution: `/wunder`

Suits a one-off task that needs no session and a quick first try at the capabilities. Give it a user identity and a question and you get the result directly, without creating a session first.

```bash
curl -X POST http://localhost:18000/wunder \
  -H "Content-Type: application/json" \
  -d '{
    "user_id": "test-user",
    "question": "Write me a Hello World"
  }'
```

### Chat Control: `/wunder/chat/*` + WebSocket

Use this set for a full chat product that needs history and real-time interaction. Creating a session, sending messages, and recovering after a disconnect all live here.

Connection order:

```
1. Create or find a session → /wunder/chat/sessions
2. Open the WebSocket → /wunder/chat/ws
3. Send messages over the WebSocket
4. Receive events over the WebSocket
```

### File Artifacts: Workspace API

Uploads, downloads, directory browsing, and artifact hand-back all go through the workspace. Each user maps to exactly one fixed cloud directory, and paths are always workspace-relative, never exposing the server's real directory.

## Use WebSocket for Real-Time Channels

Chat and desktop real-time sessions all use `/wunder/chat/ws`. SSE is reserved for non-chat interfaces such as A2A and admin diagnostics; chat should no longer implement an SSE fallback.

## A Few Conventions That Are Easy to Miss

- **The user identity does not need to be registered first.** `user_id` can be any identifier; registered users are only needed for login and permission scenarios.
- **The token count is context usage, not a bill.** Reading the current context usage and reading the cumulative consumption are two different numbers.
- **A thread's prompt is fixed once determined.** Later rounds do not rewrite it, so the model's prompt cache is not broken repeatedly.
- **Long-term memory is injected only once, at thread initialization**, and is not rewritten automatically afterward; call the memory tools when you need to.

## Common Points of Confusion

- `/wunder` is an execution entry, not a complete chat solution.
- Chat's real-time state goes only over WebSocket; after a disconnect, reconnect first, then catch up on events or observe the session again.
- The workspace and the temporary directory have different responsibilities; do not mix them.
- You do not need to register a user before calling.

## Next Steps

- Every endpoint → [API Index](/docs/en/reference/api-index/)
- Event semantics → [Stream Events Reference](/docs/en/reference/stream-events/)
- Failures and diagnostics → [Troubleshooting](/docs/en/help/troubleshooting/)
