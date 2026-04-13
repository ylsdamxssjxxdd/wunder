---
title: Integration Overview
summary: Wunder's integration surface includes a unified execution entry, chat domain, workspace, channels, A2A, and MCP; pick the right layer first for a more stable integration.
read_when:
  - You are about to integrate Wunder into your business system
  - You are not sure whether to start with /wunder, the chat domain, or the workspace API
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# Integration Overview

The key to integrating with Wunder is not "memorizing every endpoint" but **choosing the right domain first**.

---

## Choose First: What Are You Trying to Do?

| Your Scenario | Recommended Entry Point | Why? |
|----------|----------|----------|
| One-off execution, lightweight integration | [wunder API](/docs/en/integration/wunder-api/) | Shortest path, single request yields a result |
| Chat product, session management | [Chat Sessions](/docs/en/integration/chat-sessions/) + [Chat WebSocket](/docs/en/integration/chat-ws/) | Full session lifecycle + real-time push |
| File management, artifact persistence | [Workspace API](/docs/en/integration/workspace-api/) | Unified upload, download, and routing |
| Document conversion, temporary downloads | [Temp Directory](/docs/en/integration/temp-dir/) | doc/pdf to md, temporary file sharing |
| External channel integration | [Channel Webhook](/docs/en/integration/channel-webhook/) | Unified entry for Feishu, WeChat, QQ, XMPP |
| WeChat channel selection (new vs. legacy) | [WeChat iLink Channel](/docs/en/integration/weixin-channel/) | Quickly distinguish `weixin` from `wechat/wechat_mp` and get the QR-code onboarding path |
| System embedding, SSO-free access | [External Login](/docs/en/integration/external-login/) | Embed into your system without requiring a separate login |
| Agent-to-agent interconnection | [A2A Interface](/docs/en/integration/a2a/) | Standard protocol for cross-system collaboration |
| Extending the tool ecosystem | [MCP Endpoint](/docs/en/integration/mcp-endpoint/) | Mount third-party MCP services |

---

## Integration Surface Overview

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/integration/wunder-api/">
    <strong>wunder API</strong>
    <span>Shortest-path execution entry point.</span>
  </a>
  <a class="docs-card" href="/docs/en/integration/chat-sessions/">
    <strong>Chat Sessions</strong>
    <span>Session lifecycle and messaging interface.</span>
  </a>
  <a class="docs-card" href="/docs/en/integration/chat-ws/">
    <strong>Chat WebSocket</strong>
    <span>Real-time session main channel.</span>
  </a>
  <a class="docs-card" href="/docs/en/integration/workspace-api/">
    <strong>Workspace API</strong>
    <span>Files, upload, write, routing.</span>
  </a>
  <a class="docs-card" href="/docs/en/integration/temp-dir/">
    <strong>Temp Directory</strong>
    <span>Document conversion and download pipeline.</span>
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
    <span>New WeChat channel onboarding, QR login, and troubleshooting.</span>
  </a>
  <a class="docs-card" href="/docs/en/integration/external-login/">
    <strong>External Login</strong>
    <span>SSO-free linking and embedding.</span>
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

---

## Three High-Frequency Entry Points in Detail

### 1. Unified Execution Entry: `/wunder`

**Best for**:
- One-off task execution
- No session management needed
- Quick capability validation

**Characteristics**:
- Input: `user_id` + `question`
- Output: streaming events + final result
- No need to create a session first

**Example**:
```bash
curl -X POST http://localhost:18000/wunder \
  -H "Content-Type: application/json" \
  -d '{
    "user_id": "test-user",
    "question": "帮我写一个 Hello World"
  }'
```

---

### 2. Chat Control Entry: `/wunder/chat/*` + WebSocket

**Best for**:
- Full chat products
- Session history required
- Real-time interactive experience

**Characteristics**:
- Session lifecycle management
- WebSocket first, SSE as fallback
- Supports reconnection recovery

**Onboarding flow**:
```
1. Create/get session -> /wunder/chat/sessions
2. Open WebSocket -> /wunder/chat/ws
3. Send message -> via WS
4. Receive events -> via WS
```

---

### 3. File Artifact Entry: Workspace API

**Best for**:
- File upload/download
- Artifact management
- Workspace routing

**Characteristics**:
- Isolated by `user_id` + `container_id`
- Supports path mapping
- Unified file operation interface

---

## Transport Protocol: WebSocket First, SSE as Fallback

| Protocol | Pros | Cons | Use Case |
|------|------|------|----------|
| **WebSocket** | Low latency, bidirectional, real-time | Requires persistent connection | Chat UI, real-time collaboration |
| **SSE** | Simple, good compatibility | Unidirectional, higher latency | Fallback, simple consumption |

**Recommendation**: Implement WebSocket first, use SSE as a degradation path.

---

## Key Design Conventions

### user_id Does Not Require Registration

- You can pass any virtual identifier
- Registered users are only needed for login and permission management
- External systems can freely pass in user_id

### Token Counting Reflects Context Usage

- It is not directly equivalent to billing
- Current context usage is best viewed via `round_usage.total_tokens`
- Total consumption is the sum of `round_usage.total_tokens` across all requests

### Thread System Prompts Are Frozen

- Locked after first determination
- Not rewritten in subsequent turns
- Avoids breaking LLM API prompt caching

### Long-Term Memory Is Injected Only Once

- Injected only during thread initialization
- Not automatically rewritten in subsequent turns
- Actively call memory management tools when needed

---

## Common Misconceptions

| Misconception | Correct Understanding |
|------|----------|
| `/wunder` can replace the chat domain | `/wunder` is an execution entry, not a complete chat solution |
| SSE alone is sufficient | WebSocket is recommended first for a better experience |
| `workspaces` and `temp_dir` are interchangeable | They serve different purposes with different storage strategies |
| Users must be registered before calling | user_id can be any virtual identifier |

---

## Next Steps

- Want to see all APIs? -> [API Index](/docs/en/reference/api-index/)
- Want to understand events? -> [Stream Events Reference](/docs/en/reference/stream-events/)
- Running into issues? -> [Troubleshooting](/docs/en/help/troubleshooting/)
