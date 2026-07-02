# 聊天消息呈现根治方案

## 一、背景与问题现象

用户侧前端（蜂巢）聊天页面在长期使用中反复出现以下三类问题，前期前后端多次重构优化仍未彻底解决，距离上线存在差距：

| 现象 | 表现 |
|------|------|
| 消息重复 | 用户消息或智能体消息在同会话内出现两次甚至多次 |
| 顺序错乱 | 消息气泡顺序与实际发生顺序不一致，或流式 delta 拼接错位 |
| 假死 | 发送消息后界面长时间无更新，必须刷新页面才能恢复正常 |

本方案基于对前后端消息链路的完整追踪，定位出 6 个核心根因，给出分阶段根治措施。

---

## 二、根因全景

按影响面与严重度排序：

| 编号 | 根因 | 维度 | 主要现象 | 严重度 |
|------|------|------|----------|--------|
| R1 | 排队任务执行时只 drain 不推送 | 后端 | 假死 | 致命 |
| R2 | 每轮次开始清空 stream_events | 后端 | 乱序/重复 | 致命 |
| R3 | Legacy 渲染路径无 message_id upsert | 前端 | 重复/乱序 | 高 |
| R4 | Assistant placeholder 无稳定 key | 前端 | 闪烁/假死/乱序 | 高 |
| R5 | Watch 路径 user message 插入无 event_id 去重 | 前端 | 重复 | 中高 |
| R6 | queued 与 queue_enter 事件双发 | 衔接 | 重复 | 中 |

三个维度的关系：后端协议缺陷（R1/R2）决定了前端能否拿到正确的事件流；前端状态模型缺陷（R3/R4）决定了拿到正确事件后能否正确渲染；衔接缺陷（R5/R6）是前后端字段约定不一致导致的边界问题。

---

## 三、逐根因分析与根治方案

### R1：排队任务执行时只 drain 不推送（假死头号根因）

#### 现状

会话排队时，`chat_ws.rs` 收到 `ThreadSubmitOutcome::Queued` 后只发送一个 `queued` 事件就 `continue`，不启动任何流式转发任务：

- [chat_ws.rs:478-496](file:///c:/Users/sjxx/Desktop/wunder/crates/wunder-runtime/src/api/chat_ws.rs#L478-L496) 发送 `queued` 后 `continue`
- [runtime.rs:1100-1114](file:///c:/Users/sjxx/Desktop/wunder/crates/wunder-runtime/src/services/runtime/thread/runtime.rs#L1100-L1114) `execute_task` 中 `while let Some(item) = stream.next().await` 只 drain 事件，不向 WS 投递

任务被 dispatch 执行时，事件被 `stream_persist` 持久化到 storage，但 WS 端没有任何任务在投递这些事件。前端收到 `queued` 后没有任何推送通道打开，界面长时间无变化，表现为假死。

#### 根治方案

排队任务执行期间，自动为该会话启动 WS 转发通道。复用已有的 `resume_stream_events` 轮询逻辑，在 `chat_ws.rs` 的 `Queued` 分支中注册一个后台 poll 任务：

```rust
// chat_ws.rs: Queued 分支
ThreadSubmitOutcome::Queued(info) => {
    // 发送 queued 事件（统一为持久化版本，见 R6）
    send_ws_event(&ws_tx, Some(&request_id), queued_event).await;
    // 自动启动 resume 轮询，直到任务完成
    spawn_auto_resume_for_queued_task(
        state.clone(),
        session_id.clone(),
        user.user_id.clone(),
        ws_tx.clone(),
        cancel.clone(),
        info.task_id,
    );
    continue;
}
```

`spawn_auto_resume_for_queued_task` 的核心职责：

1. 复用 `resume_stream_events`（[ws_helpers.rs:573-680](file:///c:/Users/sjxx/Desktop/wunder/crates/wunder-runtime/src/api/ws_helpers.rs#L573-L680)）的轮询逻辑，从 `stream_events` 表按 `event_id > after_event_id` 拉取增量并转发给 WS
2. 监听任务状态，收到 `queue_finish` 事件或 WS 断开时退出
3. 轮询间隔与现有 watch 一致，避免额外 DB 压力

退出条件需谨慎处理：`resume`（keep_alive=false）可能因 monitor 状态窗口提前误判 `!running` 而退出（[ws_helpers.rs:599-609,640-658](file:///c:/Users/sjxx/Desktop/wunder/crates/wunder-runtime/src/api/ws_helpers.rs#L599-L658)）。自动 resume 任务应改为 `keep_alive=true`，并显式监听 `queue_finish` 事件作为退出信号。

#### 性能考量

排队任务通常只有 1-2 个，轮询 `stream_events` 表的开销与现有 watch 路径等价，不会造成 DB 压力。

---

### R2：每轮次开始清空 stream_events（乱序/重复头号根因）

#### 现状

[execute.rs:107-121](file:///c:/Users/sjxx/Desktop/wunder/crates/wunder-runtime/src/orchestrator/execute.rs#L107-L121) 在非 admin 流式请求开始时，删除该会话的全部 `stream_events`：

```rust
if prepared.stream && !is_admin && !skip_stream_clear {
    let cleanup_session = session_id.clone();
    let storage = self.storage.clone();
    match crate::core::blocking::run_db(
        "orchestrator.execute.clear_stream",
        move || storage.delete_stream_events_by_session(&cleanup_session),
    ).await { ... }
}
```

这导致 `stream_events` 实际是轮次级 ephemeral，不是会话级 durable。前端 `resume(after_event_id=300)` 在新轮次中会跳号（301-500 已被删除），event_id 缺口对前端不可见，前端把新轮次的事件误拼到旧状态上，表现为乱序与重复。

`request.rs:171-185` 的 `start_event_id` 在清空之前读取（如 500），emitter 从 501 开始发，storage 中 1-500 全被删，501+ 存在。前端 `resume(after_event_id=300)` 拿到 501+，中间 301-500 的缺口对前端不可见。

#### 根治方案

**移除 `execute.rs:107-121` 的清空逻辑，依赖已有 TTL 清理。**

stream_events 已经有 TTL 3600s + 60s 周期清理（[constants.rs:27](file:///c:/Users/sjxx/Desktop/wunder/crates/wunder-runtime/src/orchestrator/constants.rs#L27)），无需手动清空。直接删除这段代码即可：

```rust
// 删除 execute.rs:107-121 的整段清空逻辑
// if prepared.stream && !is_admin && !skip_stream_clear {
//     ...
// }
```

保留 `skip_stream_clear` 参数以备特殊场景（如 admin 显式清理），但默认路径不再清空。

#### 性能考量

- 每 session 最多多保留 1 小时内的旧事件，单会话增量约几十 KB
- TTL 清理周期 60s 已存在，自动回收
- 对百万行级系统可接受；如需更激进清理，可缩短 TTL 或按 session 活跃度分级

#### 备选方案（不推荐）

若必须清空旧事件，改为 `DELETE FROM stream_events WHERE session_id = ? AND event_id < ?`（只删当前 `start_event_id` 之前的），保留当前轮次完整性。但仍有小窗口问题，且无法解决跨轮次 resume 的连续性。

---

### R3：Legacy 渲染路径无 message_id upsert

#### 现状

前端存在两套并行的消息状态：

| 路径 | 状态载体 | 默认渲染 | 去重/乱序能力 |
|------|----------|----------|----------------|
| Legacy | `store.messages` 数组（直接 mutate） | 是 | 启发式（content+timestamp+stream_event_id） |
| Runtime Projection | `store.runtimeProjection` | 否（仅 shadow） | 严格（event_id + event_seq + client_message_id） |

[chatRuntimeRenderAdapter.ts:58-70](file:///c:/Users/sjxx/Desktop/wunder/frontend/src/realtime/chat/chatRuntimeRenderAdapter.ts#L58-L70) 的 `resolveChatRuntimeProjectionRenderMode()` 默认返回 `'legacy'`。Projection 路径那套精密的 event_id 去重、event_seq 乱序缓冲、client_message_id 合并、snapshot 全量替换，默认情况下并不支配渲染。

Legacy `this.messages` 全程靠 `push/splice` + 启发式去重，没有 `messageById` 索引：

- [chatSendActions.ts:227-229](file:///c:/Users/sjxx/Desktop/wunder/frontend/src/stores/chatSendActions.ts#L227-L229)（bootstrap）和 [279-283](file:///c:/Users/sjxx/Desktop/wunder/frontend/src/stores/chatSendActions.ts#L279-L283)（正常）：`this.messages.push(userMessage)` + `this.messages.push(assistantMessageRaw)`
- [chatWatcher.ts:342](file:///c:/Users/sjxx/Desktop/wunder/frontend/src/stores/chatWatcher.ts#L342)：`sessionMessagesRef.push(assistantMessage)`
- [chatRealtimeMessageProtection.ts:250](file:///c:/Users/sjxx/Desktop/wunder/frontend/src/stores/chatRealtimeMessageProtection.ts#L250)：`messages.splice(insertIndex, 0, nextMessage)`

所有"重复/错乱"问题的本质是 legacy 路径没有投影层那套 client_message_id 合并机制。

#### 根治方案

**将默认 renderMode 切换为 `projection`，让 Projection 路径成为渲染真相源。**

具体步骤：

1. 修改 `resolveChatRuntimeProjectionRenderMode()` 默认返回 `'projection'`（[chatRuntimeRenderAdapter.ts:69](file:///c:/Users/sjxx/Desktop/wunder/frontend/src/realtime/chat/chatRuntimeRenderAdapter.ts#L69)）

2. 补齐 projection 渲染路径当前缺失的能力：
   - 确认 `materializeChatRuntimeMessage` 覆盖了 legacy 路径的所有字段（`workflowItems`、`stats`、`reasoning`、`stream_round`、`waiting_*` 等）
   - 确认 `selectVisibleMessageProjections` 的排序逻辑与 legacy 一致
   - 确认 `buildChatRuntimeRenderableMessages`（[chatRuntimeRenderAdapter.ts:80-98](file:///c:/Users/sjxx/Desktop/wunder/frontend/src/realtime/chat/chatRuntimeRenderAdapter.ts#L80-L98)）的 `shouldRenderMessage` 过滤与 legacy 一致

3. Legacy 路径保留作为降级方案（flag 切回即可）

#### 灰度策略

先用 `shadow` 模式在线上对比一段时间（已有 `isChatRuntimeProjectionRenderShadowEnabled` 机制和 `projection-legacy-drift` 检测），确认 projection 与 legacy 输出一致后，再切默认值。建议：

- 阶段一：默认 `shadow`，观察 drift 日志 1-2 天
- 阶段二：默认 `projection`，保留 `legacy` 作为 fallback flag
- 阶段三：移除 legacy 渲染分支（可选，待 projection 稳定后）

#### 性能考量

Projection reducer 是同步纯函数，O(n) 复杂度，性能优于 legacy 的多次启发式扫描。`materializeChatRuntimeMessage` 的开销与 legacy 构建渲染对象等价。

---

### R4：Assistant placeholder 无稳定 key

#### 现状

[chatSendActions.ts:211-222](file:///c:/Users/sjxx/Desktop/wunder/frontend/src/stores/chatSendActions.ts#L211-L222) 创建的 `assistantMessageRaw` 由 `buildMessage('assistant', '')` 构造，[274-277](file:///c:/Users/sjxx/Desktop/wunder/frontend/src/stores/chatSendActions.ts#L274-L277) 只补了 `user_turn_id`/`model_turn_id`，没有 `message_id`。

[chatRuntimeMessageKeys.ts:18-29](file:///c:/Users/sjxx/Desktop/wunder/frontend/src/realtime/chat/chatRuntimeMessageKeys.ts#L18-L29) 的 key 解析逻辑：

```typescript
export const resolveChatRuntimeRenderableKey = (
  message: ChatMessageLike | null | undefined,
  fallbackIndex?: number
): string => {
  const runtimeKey = firstText(message?.__runtime_render_key);
  if (runtimeKey) return runtimeKey;
  const role = normalizeChatRuntimeMessageKeyRole(message?.role);
  const id = resolveStableChatRuntimeMessageId(message);
  if (id) return `runtime:${role}:${id}`;
  const safeIndex = Number.isFinite(fallbackIndex) ? Math.max(0, Math.trunc(Number(fallbackIndex))) : 0;
  return `legacy:${role}:${safeIndex}`;  // 退化为数组下标
};
```

placeholder 无 message_id 时，key 退化为 `legacy:assistant:${index}`。任何 prepend/splice 都会让后续消息下标偏移，触发全量 remount：

- [chatHistoryBackfill.ts:86-95](file:///c:/Users/sjxx/Desktop/wunder/frontend/src/stores/chatHistoryBackfill.ts#L86-L95) `prependHistoryBackfillPage` 把旧消息插到前面，所有消息下标后移
- [chatWatchChannelMessageRuntime.ts:301](file:///c:/Users/sjxx/Desktop/wunder/frontend/src/stores/chatWatchChannelMessageRuntime.ts#L301) 在 pending assistant 前插入 user message，后续下标偏移

全量 remount 轻则闪烁，重则 streaming 状态丢失（组件内部 state 被 reset），表现为假死。

#### 根治方案

给 assistant placeholder 立即分配基于 `model_turn_id` 的稳定 message_id：

```typescript
// chatSendActions.ts，在 Object.assign(assistantMessageRaw) 时补充
Object.assign(assistantMessageRaw as Record<string, unknown>, {
    user_turn_id: localUserTurnId,
    model_turn_id: localModelTurnId,
    message_id: `local-assistant:${localModelTurnId}`,        // 新增：稳定 key
    client_message_id: `local-assistant:${localModelTurnId}`, // 新增：用于 reducer 合并
});
```

这样 `resolveChatRuntimeRenderableKey` 会优先用 `runtime:assistant:local-assistant:${modelTurnId}`，不再退化到 index-based key。后端 `assistant_message_created` 事件回来时，reducer 的 `resolveAssistantMessageIdForTurn`（[chatRuntimeReducer.ts:2094-2106](file:///c:/Users/sjxx/Desktop/wunder/frontend/src/realtime/chat/chatRuntimeReducer.ts#L2094-L2106)）会按 `model_turn_id` 匹配并复用 id。

#### 性能考量

零性能开销，反而减少了 Vue 的 DOM diff 工作量（stable key 让 Vue 复用 DOM 节点）。

---

### R5：Watch 路径 user message 插入无 event_id 去重

#### 现状

[chatWatcher.ts:899-907](file:///c:/Users/sjxx/Desktop/wunder/frontend/src/stores/chatWatcher.ts#L899-L907) 当 watch 收到 `round_start`/`received` 且 `extractWatchUserContent` 提取到内容时，调 `insertWatchUserMessage`。去重守卫 `shouldInsertWatchUserMessage`（[chatRuntimeControls.ts:597-613](file:///c:/Users/sjxx/Desktop/wunder/frontend/src/stores/chatRuntimeControls.ts#L597-L613)）只比较最后一条 user message 的 content 完全相等 + timestamp 在 `WATCH_USER_MESSAGE_DEDUP_MS` 窗口内。

问题：
- 后端回传的 content 与本地有任意空白/编码差异 → 去重失败 → 重复 user message
- send stream 关闭（`closeOnFinal:true`）后 watch 重新拉取，若 event id 衔接有重叠，旧 `round_start` 被重放 → 重复
- `round_start`/`received` 走 `insertWatchUserMessage`，没有 event_id 去重（channel_message 类型有 stream_event_id 去重，但 round_start 没有）

#### 根治方案

**后端侧**：在 `progress` 事件的 `question` 旁加 `client_message_id` 字段（[execute.rs:162-176](file:///c:/Users/sjxx/Desktop/wunder/crates/wunder-runtime/src/orchestrator/execute.rs#L162-L176)），让前端能按 client_message_id 匹配已有 user message。

**前端侧**：`shouldInsertWatchUserMessage` 改为优先按 `client_message_id` 匹配：

```typescript
// chatRuntimeControls.ts shouldInsertWatchUserMessage 改造
const incomingClientId = incomingMessage?.client_message_id;
if (incomingClientId) {
    const existingByClientId = messages.find(
        m => m.role === 'user' && m.client_message_id === incomingClientId
    );
    if (existingByClientId) return false;
}
// fallback to content+timestamp（保留现有逻辑作为兜底）
```

同时给 `round_start`/`received` 事件携带 `event_id`，前端按 event_id 去重，与 channel_message 路径对齐。

---

### R6：queued 与 queue_enter 事件双发

#### 现状

- [chat_ws.rs:487-494](file:///c:/Users/sjxx/Desktop/wunder/crates/wunder-runtime/src/api/chat_ws.rs#L487-L494) 收到 `Queued` 时直接构造 `StreamEvent { event: "queued", id: None, ... }` 走 `send_ws_event`，未走 EventEmitter，未持久化，无 event_id
- [runtime.rs:760-774](file:///c:/Users/sjxx/Desktop/wunder/crates/wunder-runtime/src/services/runtime/thread/runtime.rs#L760-L774) 的 `enqueue_task` 调用 `emit_queue_event("queue_enter", ...)`，走 `StreamEventService.append_event` 持久化，有 event_id

前端会先收到无 id 的 `queued`，随后（resume 时）又收到带 id 的 `queue_enter`，语义重叠。前端若不按 event_id 去重会当作两个事件处理，表现为排队提示重复。

#### 根治方案

去掉 `chat_ws.rs` 中的即时 `queued` 事件，统一用 `queue_enter`（有 event_id，可持久化，可去重）。

如果需要即时反馈（`queue_enter` 走持久化路径有延迟），可以在 `queue_enter` 事件中加 `"immediate": true` 字段标识，前端收到后立即显示排队提示，无需等待两个事件。

```rust
// chat_ws.rs: Queued 分支改造
ThreadSubmitOutcome::Queued(info) => {
    // 不再发送即时 queued 事件，统一由 emit_queue_event 发送 queue_enter
    // 自动启动 resume（见 R1）
    spawn_auto_resume_for_queued_task(...);
    continue;
}
```

---

## 四、实施优先级

| 优先级 | 根因 | 改动范围 | 预期收益 | 依赖关系 |
|--------|------|----------|----------|----------|
| P0 | R2（清空 stream_events→乱序） | 后端 `execute.rs` 删除 5 行 | 消除 resume 跳号 | 无 |
| P0 | R1（排队不推送→假死） | 后端 `chat_ws.rs` + `ws_helpers.rs` | 消除排队场景假死 | 依赖 R2（否则 resume 仍跳号） |
| P1 | R4（placeholder 无 key→乱序/假死） | 前端 `chatSendActions.ts` 改 2 行 | 消除 index-based key | 无 |
| P1 | R6（queued 双发） | 后端 `chat_ws.rs` 改几行 | 消除排队事件重复 | 与 R1 同步改 |
| P2 | R5（watch user message 去重） | 前后端各改几行 | 消除 watch 路径重复 | 依赖 R2 |
| P2 | R3（切 projection 渲染） | 前端 `chatRuntimeRenderAdapter.ts` + 补齐 projection | 根治重复/乱序 | 建议 R4/R5 完成后再切 |

建议执行顺序：R2 → R1 + R6 → R4 → R5 → R3。

R2 是最小改动（删 5 行）但收益最大，应最先完成。R1 + R6 同属排队链路，一起改。R4 是前端 2 行改动，立即生效。R5 需前后端协同。R3 是最终收敛步骤，需在前面根因修复后灰度切换。

---

## 五、性能与稳定性考量

### 存储开销

- R2 不清空 stream_events：每 session 最多多保留 1 小时内的旧事件，单会话增量约几十 KB，TTL 自动清理
- R1 自动 resume：轮询 `stream_events` 表，间隔与现有 watch 一致，排队任务通常 1-2 个，无 DB 压力

### 前端性能

- R3 切 projection：reducer 是同步纯函数 O(n)，优于 legacy 的多次启发式扫描
- R4 stable key：零开销，反而减少 Vue DOM diff 工作量
- R5 client_message_id 去重：O(n) 查找，可后续优化为 Map 索引

### 稳定性

- R1 自动 resume 的退出条件需谨慎：用 `keep_alive=true` + 显式监听 `queue_finish`，避免 monitor 状态窗口导致提前退出
- R3 切 projection 前必须经过 shadow 模式验证，确认 drift 为零
- 所有改动保留 fallback flag，可快速回退

### 并发与一致性

- 后端 lease 机制（[runtime.rs:60-84](file:///c:/Users/sjxx/Desktop/wunder/crates/wunder-runtime/src/services/runtime/thread/runtime.rs#L60-L84)）保证同会话同时刻只有一个 lease，不会因并发导致事件乱序
- `should_attempt_task`（[runtime.rs:1025-1038](file:///c:/Users/sjxx/Desktop/wunder/crates/wunder-runtime/src/services/runtime/thread/runtime.rs#L1025-L1038)）保证同 thread 不并发执行
- 跨会话 fork 是隔离的，各自有独立 event_id 序列

---

## 六、验证策略

### 单元验证

| 根因 | 验证场景 | 预期结果 |
|------|----------|----------|
| R2 | 发送消息→断网→重连→resume | event_id 不跳号，状态连续 |
| R1 | 开排队→发消息→收 queued | 自动收到后续流式事件，不假死 |
| R6 | 开排队→发消息 | 只收到一次排队提示 |
| R4 | DevTools 检查 assistant placeholder key | key 不含 index，prepend 不触发 remount |
| R5 | watch 路径收到 round_start | 不重复插入 user message |
| R3 | shadow 模式对比 projection vs legacy | drift 日志为零 |

### 回归测试

- 前端：`test:chat-realtime`、`test:chat-*`、Playwright e2e
- 后端：`cargo test -j 8 -p wunder-runtime`（触及 stream_events、thread runtime、ws helpers）
- 覆盖 PostgreSQL 与 SQLite 双后端

### 手工验证清单

1. 普通发送：消息不重复、顺序正确、流式更新流畅
2. 排队发送：收到排队提示→自动收到执行事件→完成
3. 断线重连：状态恢复正确，不跳号，不重复
4. 历史回填：向上滚动加载旧消息，现有消息不闪烁、不丢失流式状态
5. 路由切换：切走再切回，状态正确，无双 watcher 竞争
6. 长会话：连续发送 50+ 条消息，无累积延迟或假死

---

## 七、风险与回退

### 风险

| 风险 | 概率 | 缓解措施 |
|------|------|----------|
| R2 不清空导致 storage 增长 | 低 | TTL 3600s 已存在；可缩短 TTL 或按 session 活跃度分级 |
| R1 自动 resume 与现有 watch 竞争 | 中 | 共享 watchController 守卫，互斥运行 |
| R3 projection 渲染字段缺失 | 中 | shadow 模式验证；保留 legacy fallback flag |
| R4 client_message_id 与后端冲突 | 低 | 用 `local-assistant:` 前缀，与后端 id 命名空间隔离 |

### 回退

每个根因修复都可通过 flag 回退：

- R2：恢复 `execute.rs` 清空逻辑
- R1：禁用 `spawn_auto_resume_for_queued_task`，回退到手动 resume
- R3：`chat_runtime_render=legacy` 切回 legacy 渲染
- R4：移除 placeholder 的 message_id 赋值
- R5/R6：恢复原事件发送逻辑

---

## 八、关键文件索引

### 后端

| 职责 | 文件 |
|------|------|
| 每轮清空 stream_events（R2） | `crates/wunder-runtime/src/orchestrator/execute.rs:107-121` |
| 排队任务 drain 不推送（R1） | `crates/wunder-runtime/src/services/runtime/thread/runtime.rs:1100-1114` |
| queued 事件发送（R1/R6） | `crates/wunder-runtime/src/api/chat_ws.rs:478-496` |
| queue_enter 事件持久化（R6） | `crates/wunder-runtime/src/services/runtime/thread/runtime.rs:760-774` |
| resume_stream_events 轮询（R1） | `crates/wunder-runtime/src/api/ws_helpers.rs:573-680` |
| stream_events TTL 清理 | `crates/wunder-runtime/src/orchestrator/constants.rs:27` |
| EventEmitter event_id 分配 | `crates/wunder-runtime/src/orchestrator/event_stream.rs:197-245,338-356` |
| stream_events 表 schema | `crates/wunder-runtime/src/storage/sqlite/schema.rs:500-507` |
| stream_events 持久化（SQLite） | `crates/wunder-runtime/src/storage/sqlite/agent_runtime_store.rs:354-501` |
| stream_events 持久化（Postgres） | `crates/wunder-runtime/src/storage/postgres/agent_runtime_store.rs:336-478` |
| progress 事件携带 question（R5） | `crates/wunder-runtime/src/orchestrator/execute.rs:162-176` |

### 前端

| 职责 | 文件 |
|------|------|
| 默认渲染模式（R3） | `frontend/src/realtime/chat/chatRuntimeRenderAdapter.ts:58-70` |
| Assistant placeholder 创建（R4） | `frontend/src/stores/chatSendActions.ts:211-222,274-277` |
| Stable key 解析（R4） | `frontend/src/realtime/chat/chatRuntimeMessageKeys.ts:18-29` |
| Watch user message 插入去重（R5） | `frontend/src/stores/chatRuntimeControls.ts:597-613` |
| Watch 路径 round_start 处理（R5） | `frontend/src/stores/chatWatcher.ts:899-907` |
| Runtime reducer（event_id/seq 去重乱序） | `frontend/src/realtime/chat/chatRuntimeReducer.ts` |
| 渲染列表 computed（legacy 优先） | `frontend/src/views/messenger/controller/messengerControllerRenderableMessages.ts:724-777` |
| 历史回填 prepend | `frontend/src/stores/chatHistoryBackfill.ts:86-95` |
| Watch channel 消息处理 | `frontend/src/stores/chatWatchChannelMessageRuntime.ts` |
| 数组引用保持替换 | `frontend/src/stores/chatMessageArraySync.ts` |
| Watchdog tick | `frontend/src/stores/chatWatcher.ts:573-664` |

---

## 九、附录：未列入本次根治的次要问题

以下问题影响较小或属于上述根因的衍生症状，可在根治完成后作为后续优化：

1. **watchdog 在 send 活跃时跳过 reconcile**：[chatWatcher.ts:579-582](file:///c:/Users/sjxx/Desktop/wunder/frontend/src/stores/chatWatcher.ts#L579-L582) 已有 `sendController` 守卫，当前实现基本安全，但 `settleTerminalAssistantArtifactsBase` 在 `running === false` 时仍可能触发，需观察是否有提前 finalize 情况
2. **80ms watcher 自动重启可能产生双 watcher**：[chatWatcher.ts:1034](file:///c:/Users/sjxx/Desktop/wunder/frontend/src/stores/chatWatcher.ts#L1034) 的 `setTimeout` 在快速路由切换时可能与新 watcher 竞争，重启前应检查 `sessionWatchSessionId === key` 且无活跃 watchController
3. **send 路径 sessionMessagesRef 不重新解析**：[chatSendActions.ts:281](file:///c:/Users/sjxx/Desktop/wunder/frontend/src/stores/chatSendActions.ts#L281) 一次性捕获引用，非活跃 session 切换 cached 数组时可能写入 detached 数组。切到 projection 渲染后此问题自动消失
4. **慢客户端 delta 丢弃**：[ws_helpers.rs:331-334,433-446](file:///c:/Users/sjxx/Desktop/wunder/crates/wunder-runtime/src/api/ws_helpers.rs#L331-L334) delta 满队列时丢弃，依赖前端 resume 补全。R2 修复后 resume 可靠性提升，此问题缓解
5. **resume 轮询延迟**：[ws_helpers.rs:585,660-673](file:///c:/Users/sjxx/Desktop/wunder/crates/wunder-runtime/src/api/ws_helpers.rs#L585-L673) resume/watch 是轮询非 push，事件到达有延迟。可后续改为 push 通知 + 轮询兜底

---

本方案基于静态代码分析，未运行时验证。建议在实施前用 TRAE-debugger 收集运行时证据验证 R1/R2 的假死与乱序路径，确认改动后行为符合预期。
