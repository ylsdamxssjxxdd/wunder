---
title: Boundary Handling
summary: wunder's handling strategies and recovery mechanisms for edge cases such as context overflow, network interruption, tool failure, and system exceptions.
read_when:
  - You need to understand system behavior under exceptional conditions
  - You need to design fault tolerance and recovery logic
  - You need to troubleshoot production environment issues
source_docs:
  - docs/设计文档/01-系统总体设计.md
  - src/orchestrator/
  - src/core/
---

# Boundary Handling

One of wunder's design goals is to **maintain stable system operation under various boundary and exceptional conditions**, rather than simply crashing.

This page details the system's handling strategies for various boundary scenarios.

---

## 1. Context Overflow Handling

### 1.1 What is Context Overflow

Context overflow is triggered when the total token count of conversation history + current request + tool results exceeds the model's context window limit.

### 1.2 Handling Strategy

wunder adopts a **progressive context compression** strategy, rather than simple truncation:

```
Full context → Compression Strategy 1 → Compression Strategy 2 → ... → Fit model window
```

### 1.3 Compression Levels

| Compression Level | Strategy | Data Loss | Applicable Scenario |
|---------|------|---------|---------|
| **Level 0** | Keep full context | None | When context is sufficient |
| **Level 1** | Remove tool intermediate result summaries | Low | Tool results can be re-fetched |
| **Level 2** | Summarize historical messages | Medium | Preserve semantics, lose details |
| **Level 3** | Remove early rounds | High | Keep only the last N rounds |
| **Level 4** | Keep only system prompt + current request | Extreme | Emergency degradation |

### 1.4 Key Design Constraints

- **Thread Freeze Principle**: Even with context compression, the system prompt remains unchanged (frozen after initial determination)
- **Memory Isolation**: Long-term memory is injected only once during thread initialization and does not participate in dynamic compression
- **Observability**: Each compression records a `context_compaction` event, including compression strategy and loss assessment

---

## 2. Network Interruption Handling

### 2.1 WebSocket Connection Interruption

#### Client-side Behavior

| Stage | Behavior |
|------|------|
| **Connection Dropped** | Immediately show "connection lost" prompt, start auto-reconnect |
| **Reconnecting** | Show reconnect progress, exponential backoff (1s, 2s, 4s, 8s, 16s, max 32s) |
| **Reconnect Success** | Send `resume` request, fetch missed events |
| **Reconnect Failed** | After exceeding max retries, prompt for manual refresh |

#### Server-side Behavior

```
Connection dropped → Maintain session state for 300s → After timeout, clean up memory state (but retain database records)
              ↓
         Cache unpushed events
              ↓
         Resend on client reconnect
```

### 2.2 SSE Connection Interruption

SSE serves as a fallback for WebSocket with similar handling logic:

- Auto-reconnect (native browser support)
- Restore event stream via `Last-Event-ID`
- Server retains the last 1000 events for recovery

### 2.3 Model API Call Failures

| Failure Type | Retry Strategy | Fallback Plan |
|---------|---------|---------|
| **Network Timeout** | Retry 3 times, exponential backoff | Prompt user to retry later |
| **Rate Limit** | Wait for `retry-after` header, max wait 60s | Enter queue |
| **Authentication Failed** | Fail immediately, no retry | Prompt admin to check config |
| **Model Error** | Retry once (if temporary error) | Switch to fallback model (if configured) |
| **Context Overflow** | No retry, trigger compression logic | Auto-compress then retry |

---

## 3. Tool Execution Failure Handling

### 3.1 Tool Failure Categories

| Failure Category | Description | Retryable |
|---------|------|--------|
| **Parameter Error** | Tool parameter validation failed | No - model needs to fix parameters |
| **Permission Denied** | Tool not in whitelist or approval rejected | No - requires user authorization |
| **Temporary Error** | File lock, network fluctuation, etc. | Yes - auto retry |
| **System Error** | Tool execution engine exception | Yes - limited retry |

### 3.2 Tool Result Overflow Protection

To prevent tool output from becoming too large and causing context overflow, wunder implements **three-layer protection**:

```
Tool Execution → Layer 1: Execution Budget Trimming → Layer 2: MCP Secondary Compression → Layer 3: Final Check Before Model Input
```

#### Layer 1: Execution Budget

```rust
// Budget parameters for command execution tools
budget: {
  time_budget_ms: 60000,      // Max execution time
  output_budget_bytes: 4194304, // Max output bytes (4MB)
  max_commands: 200            // Max command count
}
```

#### Layer 2: Output Trimming Strategy

| Output Size | Handling |
|---------|---------|
| ≤ Budget | Return complete |
| Budget < Size ≤ 2×Budget | Keep head 50% + tail 50% |
| > 2×Budget | Keep head 25% + tail 25% + omit middle |

#### Layer 3: Signal Feedback

Trimmed results include metadata:

```json
{
  "output": "...trimmed content...",
  "output_meta": {
    "truncated": true,
    "original_size_bytes": 8388608,
    "returned_size_bytes": 4194304,
    "continuation_required": true
  }
}
```

The model can decide based on these signals to:
- Narrow query scope
- Fetch results paginated
- Accept current info and continue

### 3.3 Atomic Write Strategy

Tools involving file writes (`write_file`, `apply_patch`) use **temp file + atomic replace** strategy:

```
1. Write to temp file: file.tmp
2. fsync to ensure data is on disk
3. Atomic rename: file.tmp → file
4. On failure, delete temp file, keep original
```

This ensures:
- No half-written files
- Concurrent writes don't corrupt each other
- Rollback on failure

---

## 4. Session and Thread Exception Handling

### 4.1 Session State Machine

wunder sessions have clear state transitions:

```
idle → running → waiting_approval → waiting_user_input → idle
  ↓        ↓
error  cancelled
```

### 4.2 Exception State Recovery

| Exception State | Trigger Condition | Recovery Strategy |
|---------|---------|---------|
| **`system_error`** | Uncaught system exception | Log error, reset session to idle |
| **`cancelled`** | User actively cancels | Stop execution immediately, clean up resources |
| **`timeout`** | Execution exceeds configured time | Stop execution, return partial results (if any) |
| **`approval_pending`** | Waiting for user approval | Pause execution, wait for user decision |

### 4.3 Thread State Sync

Thread status is synced in real-time via `thread_status` events:

```json
{
  "event": "thread_status",
  "status": "running", // running, waiting_approval, waiting_user_input, idle, system_error
  "session_id": "xxx",
  "timestamp": 1234567890
}
```

---

## 5. Concurrency and Queue Handling

### 5.1 Main Thread Queue

Each `user_id + agent_id` has only one main thread; when busy, new requests enter the queue:

```
New request → Is main thread idle? → Yes → Execute immediately
              ↓ No
         Enter agent_tasks queue
              ↓
         Execute sequentially when main thread becomes idle
```

### 5.2 Queue Configuration

| Config Item | Description | Default Value |
|-------|------|--------|
| `max_queue_size` | Max queue length | 100 |
| `queue_timeout_s` | Queue timeout | 3600 |
| `max_concurrent_sessions` | Max concurrent sessions | 10 |

### 5.3 Priority Strategy

Tasks in the queue are sorted by the following priority:
1. Admin requests (highest)
2. Requests with `priority: high`
3. Normal requests
4. Requests with `priority: low`

---

## 6. Data Consistency Guarantee

### 6.1 Database Transactions

All operations involving multiple tables use database transactions:

```rust
// Pseudocode example
begin_transaction();
try {
  update_chat_session();
  insert_message();
  update_agent_thread();
  commit();
} catch {
  rollback();
  throw;
}
```

### 6.2 Idempotency Design

| Endpoint | Idempotency Key | Description |
|------|--------|------|
| `/wunder/chat/messages` | `client_msg_id` | Duplicate messages won't be processed twice |
| Channel Webhook | `message_id` | Duplicate pushes won't be processed twice |
| Tool Call | `tool_call_id` | Duplicate calls won't execute twice |

### 6.3 Eventual Consistency

For async operations (like channel message delivery), **outbox pattern** ensures eventual consistency:

```
1. Write business operation + outbox record (same transaction)
2. Background worker reads from outbox
3. Execute external operation
4. Mark outbox as complete on success
5. Retry on failure (with exponential backoff)
```

---

## 7. Resource Leak Prevention

### 7.1 Timeout Mechanisms

| Resource Type | Timeout Config | Cleanup Behavior |
|---------|---------|---------|
| Tool Execution | `tool_timeout_s` | Terminate process, release resources |
| Model Call | `llm_timeout_s` | Cancel request, release connection |
| WebSocket Connection | `ws_idle_timeout_s` | Close idle connections |
| Temp Files | `temp_dir_ttl_h` | Periodically clean expired files |

### 7.2 Connection Pool Management

- Database connections: Use connection pool, auto-reclaim idle connections
- HTTP client: Connection reuse, limit max concurrent connections
- WebSocket: Limit max connections per user

### 7.3 Memory Protection

- Tool output limits: Prevent single tool from returning oversized results
- Context compression: Auto-control memory usage
- Session history trimming: Limit history message count at database level

---

## 8. Observability and Debugging

### 8.1 Error Classification

All errors carry structured `error_meta`:

```json
{
  "error": "Operation failed",
  "error_meta": {
    "category": "tool_execution",
    "severity": "error",
    "retryable": true,
    "retry_after_ms": 5000,
    "source_stage": "tool_call",
    "recovery_action": "retry_tool"
  }
}
```

### 8.2 Key Event Logging

| Event | Recorded Content | Purpose |
|------|---------|---------|
| `context_compaction` | Compression strategy, compression ratio | Optimize context management |
| `tool_failed` | Tool name, error type, retry suggestion | Tool debugging |
| `queue_position` | Current position, estimated wait time | User experience |
| `recovery_attempt` | Recovery strategy, result | Stability analysis |

### 8.3 Log Levels

| Level | Use Case |
|------|---------|
| `ERROR` | System exceptions, requires manual intervention |
| `WARN` | Boundary conditions, auto-recovered but needs attention |
| `INFO` | Normal flow, key checkpoints |
| `DEBUG` | Detailed debugging info |

---

## 9. Common Boundary Scenarios Quick Reference

| Scenario | Expected Behavior | Reference Section |
|------|---------|---------|
| Session history too long | Auto-compress context | 1. Context Overflow Handling |
| WebSocket disconnected | Auto-reconnect + event replay | 2. Network Interruption Handling |
| Tool output too large | Trim + signal feedback | 3.2 Tool Result Overflow Protection |
| Crash during file write | Atomic write, original file preserved | 3.3 Atomic Write Strategy |
| Main thread busy | New requests queue | 5.1 Main Thread Queue |
| Network fluctuation | Exponential backoff retry | 2.3 Model API Call Failures |
| Database operation failed | Transaction rollback | 6.1 Database Transactions |

---

## Further Reading

- [Streaming Execution](/docs/en/concepts/streaming/)
- [Sessions and Rounds](/docs/en/concepts/sessions-and-rounds/)
- [Troubleshooting](/docs/en/help/troubleshooting/)
- [Monitoring and Benchmark](/docs/en/ops/benchmark-and-observability/)