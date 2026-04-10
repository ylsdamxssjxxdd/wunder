---
title: Boundary handling
summary: The handling strategies and recovery mechanisms of Wunder in boundary scenarios such as context overflow, network interruption, tool failure, and system anomalies.
read_when:
  - You need to understand the system's behavior in abnormal situations
  - You need to design fault tolerance and recovery logic
  - You need to troubleshoot the abnormal issues in the production environment.
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
  - src/orchestrator/
  - src/core/
---
# Boundary Handling
One of wunder's design goals is **to keep the system running stably under various boundary and exceptional conditions**, rather than simply crashing.
This page provides a detailed explanation of the system's handling strategies in various boundary scenarios.
---

## 1. Handling Context Overflow
### 1.1 What is Context Length Limit
When the total number of tokens in the conversation history, the current request, tool results, and other content exceeds the model's context window limit, a context overflow is triggered.
### 1.2 Handling Strategy
wunder uses a **progressive context compression** strategy, rather than simple truncation:
```
完整上下文 → 压缩策略1 → 压缩策略2 → ... → 适配模型窗口
```

### 1.3 Compression Levels
| Compression Level | Strategy | Degree of Loss | Applicable Scenarios |
|------------------|---------|----------------|------------------|
| **Level 0** | Retain full context | None | When context is sufficient |
| **Level 1** | Remove intermediate tool result summaries | Low | Tool results can be retrieved again |
| **Level 2** | Summarize historical messages | Medium | Retain semantics, lose details |
| **Level 3** | Remove early rounds | High | Only keep the most recent N rounds |
| **Level 4** | Keep only system prompt and current request | Very high | Emergency downgrade |
### 1.4 Key Design Constraints
- **Thread Freezing Principle**: Even if the context is compressed, the system prompt remains unchanged (frozen once determined for the first time)
- **Memory Isolation**: Long-term memory is injected only once during thread initialization and does not participate in dynamic compression
- **Observability**: Each compression records a `context_compaction` event, including the compression strategy and loss evaluation
---

## 2. Network Interruption Handling
### 2.1 WebSocket Connection Interruption
#### Client-Side Behavior
| Stage | Action |
|------|------|
| **Disconnected** | Immediately display "Disconnected" prompt and initiate automatic reconnection |
| **Reconnecting** | Show reconnection progress, exponential backoff (1s, 2s, 4s, 8s, 16s, max 32s) |
| **Reconnection Successful** | Send `resume` request to retrieve missed events |
| **Reconnection Failed** | After exceeding maximum retry attempts, prompt for manual refresh |
#### Server-Side Behavior
```
连接断开 → 保持会话状态 300s → 超时后清理内存状态（但保留数据库记录）
              ↓
         缓存未推送的事件
              ↓
         客户端重连时补发
```

### 2.2 SSE Connection Interruption
SSE, as a fallback solution for WebSocket, has a similar handling logic:
- Automatic reconnection (natively supported by the browser)
- Restore event stream via `Last-Event-ID`
- The server keeps the most recent 1000 events for recovery
### 2.3 Model API Call Failure
| Failure Type | Retry Strategy | Degradation Plan |
|--------------|----------------|----------------|
| **Network Timeout** | Retry 3 times, exponential backoff | Prompt the user to try again later |
| **Rate Limit** | Wait for `retry-after` header, maximum wait 60s | Enter queue |
| **Authentication Failure** | Fail immediately, do not retry | Prompt the administrator to check configuration |
| **Model Error** | Retry once (if temporary error) | Switch to backup model (if configured) |
| **Context Limit Exceeded** | Do not retry, trigger compression logic | Automatically compress and retry |
---

## 3. Tool Execution Failure Handling
### 3.1 Tool Failure Classification
| Failure Type | Description | Retryable |
|--------------|------------|----------|
| **Parameter Error** | Tool parameter validation failed | ❌ Requires model to correct parameters |
| **Permission Denied** | Tool not in the whitelist or approval denied | ❌ Requires user authorization |
| **Temporary Error** | File lock, network fluctuation, etc. | ✅ Automatic retry |
| **System Error** | Tool execution engine exception | ✅ Limited retry |
### 3.2 Tool Output Explosion Prevention Mechanism
To prevent the tool from producing excessively large outputs that could exceed the context limit, wunder has implemented **three layers of protection**:
```
工具执行 → Layer 1: 执行层预算裁剪 → Layer 2: MCP 二次压缩 → Layer 3: 入模前最终检查
```

#### Layer 1: Execution Layer Budget
```rust
// 执行命令工具的预算参数
budget: {
  time_budget_ms: 60000,      // 最大执行时间
  output_budget_bytes: 4194304, // 最大输出字节（4MB）
  max_commands: 200            // 最大命令数
}
```

#### Layer 2: Output Pruning Strategy
| Output Size | Processing Method |
|------------|----------------|
| ≤ Budget | Return in full |
| Budget < Size ≤ 2×Budget | Keep first 50% and last 50% |
| > 2×Budget | Keep first 25% and last 25%, omit the middle |
#### Layer 3: Signal Feedback
The cropped result will include metadata:
```json
{
  "output": "...裁剪后的内容...",
  "output_meta": {
    "truncated": true,
    "original_size_bytes": 8388608,
    "returned_size_bytes": 4194304,
    "continuation_required": true
  }
}
```

The model can decide based on these signals whether to: 
- Narrow the query range
- Retrieve results in pages
- Accept the current information and continue
### 3.3 Atomic Write Strategy
The tools involving file writing (`写入文件`, `应用补丁`) use a **temporary file and atomic replacement** strategy:
```
1. 写入临时文件：file.tmp
2. fsync 确保数据落盘
3. 原子重命名：file.tmp → file
4. 如果失败，删除临时文件，保留原文件
```

This ensures: 
- ✅ No partially written files
- ✅ Concurrent writes do not interfere with each other
- ✅ Rollback is possible in case of failure
---

## 4. Conversation and Thread Exception Handling
### 4.1 Conversation State Machine
Wunder's conversations have clear state transitions:
```
idle → running → waiting_approval → waiting_user_input → idle
  ↓        ↓
error  cancelled
```

### 4.2 Abnormal State Recovery
| Exception Status | Trigger Condition | Recovery Strategy |
|-----------------|-----------------|-----------------|
| **`system_error`** | Uncaught system exception | Log error, reset session to idle |
| **`cancelled`** | User-initiated cancellation | Immediately stop execution, clean up resources |
| **`timeout`** | Execution exceeds configured time | Stop execution, return partial results (if any) |
| **`approval_pending`** | Waiting for user approval | Pause execution, wait for user's decision |
### 4.3 Thread State Synchronization
Real-time synchronization of thread status through the `thread_status` event:
```json
{
  "event": "thread_status",
  "status": "running", // running, waiting_approval, waiting_user_input, idle, system_error
  "session_id": "xxx",
  "timestamp": 1234567890
}
```

---

## 5. Concurrency and Queue Processing
### 5.1 Main Thread Queueing
Each `user_id + agent_id` has only one main thread, and new requests enter the queue when busy:
```
新请求 → 主线程空闲？→ 是 → 立即执行
              ↓ 否
         进入 agent_tasks 队列
              ↓
         主线程空闲后依次执行
```

### 5.2 Queue Configuration
| Configuration Item | Description | Default Value |
|-------------------|------------|---------------|
| `max_queue_size` | Maximum Queue Length | 100 |
| `queue_timeout_s` | Queue Timeout | 3600 |
| `max_concurrent_sessions` | Maximum Concurrent Sessions | 10 |
### 5.3 Priority Strategy
Tasks in the queue are prioritized as follows:
1. Administrator requests (highest)
2. Requests with `priority: high`
3. Regular requests
4. Requests with `priority: low`
---

## 6. Data Consistency Assurance
### 6.1 Database Transactions
All operations involving multiple tables use database transactions:
```rust
// 伪代码示例
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
| Interface | Idempotent Key | Description |
|------|--------|------|
| `/wunder/chat/messages` | `client_msg_id` | Duplicate messages will not be processed again |
| Channel Webhook | `message_id` | Duplicate pushes will not be processed again |
| Tool Invocation | `tool_call_id` | Duplicate calls will not be executed again |
### 6.3 Eventual Consistency
For asynchronous operations (such as channel message delivery), use the **outbox pattern** to ensure eventual consistency:
```
1. 业务操作 + outbox 记录写入（同一事务）
2. 后台 worker 从 outbox 读取
3. 执行外部操作
4. 成功后标记 outbox 为完成
5. 失败则重试（带指数退避）
```

---

## 7. Resource Leak Protection
### 7.1 Timeout Mechanism
| Resource Type | Timeout Configuration | Cleanup Behavior |
|---------------|-------------------|----------------|
| Tool Execution | `tool_timeout_s` | Terminate process, release resources |
| Model Call | `llm_timeout_s` | Cancel request, release connection |
| WebSocket Connection | `ws_idle_timeout_s` | Close idle connection |
| Temporary File | `temp_dir_ttl_h` | Periodically clean up expired files |
### 7.2 Connection Pool Management
- Database connection: use connection pool, automatically reclaim idle connections
- HTTP client: connection reuse, limit maximum concurrency
- WebSocket: limit maximum connections per user
### 7.3 Memory Protection
- Tool output limit: Prevent a single tool from returning excessively large results
- Context compression: Automatically control memory usage
- Session history trimming: Limit the number of historical messages at the database level
---

## 8. Observability and Debugging
### 8.1 Error Classification
All errors carry a structured `error_meta`:
```json
{
  "error": "操作失败",
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

### 8.2 Key Event Records
| Event | Record Content | Purpose |
|------|---------------|---------|
| `context_compaction` | Compression strategy, compression ratio | Optimize context management |
| `tool_failed` | Tool name, error type, retry suggestion | Tool debugging |
| `queue_position` | Current position, estimated wait time | User experience |
| `recovery_attempt` | Recovery strategy, result | Stability analysis |
### 8.3 Log Classification
| Level | Usage Scenario |
|------|---------|
| `ERROR` | System exception, requires manual intervention |
| `WARN` | Boundary case, automatically recovers but needs attention |
| `INFO` | Normal process, key nodes |
| `DEBUG` | Detailed debugging information |
---

## 9. Quick Reference for Common Boundary Scenarios
| Scenario | Expected Behavior | Reference Section |
|------|---------|---------|
| Conversation history too long | Automatically compress context | 1. Context overflow handling |
| WebSocket disconnected | Auto-reconnect and event resend | 2. Network interruption handling |
| Tool output too large | Trimming and signal feedback | 3.2 Tool result explosion prevention mechanism |
| Crash when writing files | Atomic write, do not corrupt original file | 3.3 Atomic write strategy |
| Main thread busy | New requests queued | 5.1 Main thread queuing |
| Network fluctuation | Exponential backoff retry | 2.3 Model API call failure |
| Database operation failed | Transaction rollback | 6.1 Database transactions |
---

## Further Reading
- [Streaming Execution](/docs/en/concepts/streaming/)
- [Sessions and Rounds](/docs/en/concepts/sessions-and-rounds/)
- [Troubleshooting](/docs/en/help/troubleshooting/)
- [Monitoring and Benchmark](/docs/en/ops/benchmark-and-observability/)
