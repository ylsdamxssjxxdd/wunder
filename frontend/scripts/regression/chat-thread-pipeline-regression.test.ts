import test from 'node:test';
import assert from 'node:assert/strict';

import {
  toChatThreadFrame,
  applyChatThreadServerEvent,
  ensureChatThreadRuntime,
  buildChatThreadMaterializedMessages,
  markChatThreadFallback,
  registerChatThreadSnapshotLoader,
  isChatThreadV2Session,
  resetChatThreadRuntime,
  getChatThreadState
} from '../../src/realtime/chat/chatThreadRuntime';
import { composeItemText, getThreadTurn } from '../../src/realtime/chat/chatThreadState';
import {
  emptyChatThreadState,
  THREAD_GAP_MAX_FRAMES,
  THREAD_GAP_MAX_MS,
  THREAD_OVERFLOW_RESUME_COOLDOWN_MS
} from '../../src/realtime/chat/chatThreadTypes';

const SESSION = 'sess-change-stream';

class FakeStore {
  public sessionVersions: Record<string, number> = {};
  public contentVersions: Record<string, number> = {};
  public reasoningVersions: Record<string, number> = {};
  public runtimeProjectionVersionBySession: Record<string, number> = {};
  public runtimeProjectionContentVersionByMessage: Record<string, number> = {};
  public runtimeProjectionReasoningVersionByMessage: Record<string, number> = {};
  public activeSessionId = SESSION;
}

const wireChange = (data: Record<string, unknown>) => ({ event: 'thread_change', data });
const wireTail = (data: Record<string, unknown>) => ({ event: 'thread_item_tail', data });
const wireAck = (data: Record<string, unknown>) => ({ event: 'stream_started', data: { session_id: SESSION, data } });

test('v2 pipeline renders a full streaming turn from change frames', () => {
  resetChatThreadRuntime(SESSION);
  ensureChatThreadRuntime(SESSION);
  const store = new FakeStore();
  const apply = (type: string, payload: unknown) =>
    applyChatThreadServerEvent(store, SESSION, type, payload);

  // 1. send ack binds the optimistic turn — and only the optimistic turn:
  //    bubble content never travels through the ack (M2-A).
  assert.equal(apply('stream_started', wireAck({
    turn_id: 'turn-1', user_round: 1, resume_from_seq: 2,
    content: '你好', client_message_id: 'cm-1'
  })), true);
  assert.equal(getThreadTurn(getChatThreadState(SESSION)!, 'turn-1')?.userContent, null);

  // 2. session watch replays the accept-time changes (turn + user item).
  assert.equal(apply('thread_change', wireChange({
    change_type: 'turn_upsert', turn_id: 'turn-1', cursor: 1, revision: 1,
    payload: { turn_id: 'turn-1', status: 'running', user_round: 1, client_message_id: 'cm-1' }
  })), true);
  assert.equal(apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-1', cursor: 2, revision: 1,
    item: {
      item_id: 'turn-1:user', turn_id: 'turn-1', kind: 'user_message',
      status: 'completed', revision: 1, visibility: 'user',
      payload: { role: 'user', content: '你好', client_message_id: 'cm-1', user_round: 1 }
    }
  })), true);
  // userContent comes from the durable user item_upsert, not the ack.
  assert.equal(getThreadTurn(getChatThreadState(SESSION)!, 'turn-1')?.userContent, '你好');

  // 3. assistant text item registration (feeder change with embedded item).
  assert.equal(apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-1', cursor: 3, revision: 1,
    item: {
      item_id: 'turn-1:text-1', turn_id: 'turn-1', kind: 'assistant_message',
      status: 'running', revision: 1, visibility: 'user',
      payload: { model_round: 1, user_round: 1, role: 'assistant', content: '', reasoning: '' }
    }
  })), true);

  // 4. streamed text tails (send path) compose the bubble content.
  assert.equal(apply('thread_item_tail', wireTail({
    item_id: 'turn-1:text-1', field: 'content', offset: 0, text: '你好'
  })), true);
  assert.equal(apply('thread_item_tail', wireTail({
    item_id: 'turn-1:text-1', field: 'content', offset: 2, text: '，世界'
  })), true);

  // 5. tool lifecycle as durable changes.
  assert.equal(apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-1', cursor: 4, revision: 1,
    item: {
      item_id: 'turn-1:tool-call_1', turn_id: 'turn-1', kind: 'tool_call',
      status: 'running', revision: 1, visibility: 'user',
      payload: { model_round: 1, tool_call_id: 'call_1', name: 'tool', title: '工具' }
    }
  })), true);
  assert.equal(apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-1', cursor: 5, revision: 2,
    item: {
      item_id: 'turn-1:tool-call_1', turn_id: 'turn-1', kind: 'tool_call',
      status: 'completed', revision: 2, visibility: 'user',
      payload: { model_round: 1, tool_call_id: 'call_1', name: 'tool', title: '工具', detail: '完成' }
    }
  })), true);

  // 6. terminal turn.
  assert.equal(apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-1', cursor: 6, revision: 2,
    item: {
      item_id: 'turn-1:text-1', turn_id: 'turn-1', kind: 'assistant_message',
      status: 'completed', revision: 2, visibility: 'user',
      payload: { model_round: 1, role: 'assistant', content: '你好，世界', reasoning: '' }
    }
  })), true);
  assert.equal(apply('thread_change', wireChange({
    change_type: 'turn_upsert', turn_id: 'turn-1', cursor: 7, revision: 3,
    payload: { turn_id: 'turn-1', status: 'completed' }
  })), true);

  const messages = buildChatThreadMaterializedMessages(SESSION);
  assert.ok(Array.isArray(messages));
  const roles = messages.map((message) => message.role);
  assert.deepEqual(roles, ['user', 'assistant']);
  const userBubble = messages[0];
  const assistantBubble = messages[1] as Record<string, unknown>;
  assert.equal(userBubble.content, '你好');
  assert.equal(assistantBubble.content, '你好，世界');
  assert.equal(assistantBubble.status, 'final');
  const workflowItems = Array.isArray(assistantBubble.workflowItems) ? assistantBubble.workflowItems : [];
  assert.equal(workflowItems.length, 1);
  assert.equal((workflowItems[0] as Record<string, unknown>).status, 'completed');

  // 7. legacy fallthrough: interactive lifecycle frames must not be consumed.
  assert.equal(apply('approval_request', { data: { approval_id: 'a1' } }), false);
  assert.equal(apply('queued', { data: {} }), false);
  assert.equal(apply('thread_status', { data: { thread_status: 'running' } }), false);
});

test('durable pipeline reports unavailable snapshot recovery and preserves idempotency', () => {
  resetChatThreadRuntime(SESSION);
  ensureChatThreadRuntime(SESSION);
  registerChatThreadSnapshotLoader(null); // no loader wired: durable recovery callback contract
  const store = new FakeStore();
  let snapshotFallbacks = 0;
  const apply = (type: string, payload: unknown) =>
    applyChatThreadServerEvent(store, SESSION, type, payload, {
      onSnapshotRequired: () => { snapshotFallbacks += 1; }
    });

  apply('thread_change', wireChange({
    change_type: 'turn_upsert', turn_id: 'turn-2', cursor: 1, revision: 1,
    payload: { turn_id: 'turn-2', status: 'running', user_round: 1 }
  }));
  apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-2', cursor: 2, revision: 1,
    item: {
      item_id: 'turn-2:text-1', turn_id: 'turn-2', kind: 'assistant_message',
      status: 'completed', revision: 1, visibility: 'user',
      payload: { model_round: 1, content: '答案', role: 'assistant' }
    }
  }));
  const before = buildChatThreadMaterializedMessages(SESSION);
  assert.equal(before?.length, 1);

  // Duplicate frame (same cursor) must be a no-op.
  apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-2', cursor: 2, revision: 1,
    item: {
      item_id: 'turn-2:text-1', turn_id: 'turn-2', kind: 'assistant_message',
      status: 'completed', revision: 1, visibility: 'user',
      payload: { model_round: 1, content: '答案', role: 'assistant' }
    }
  }));
  const after = buildChatThreadMaterializedMessages(SESSION);
  assert.equal(after?.length, 1);
  assert.deepEqual(
    JSON.stringify(after?.map((message) => message.content)),
    JSON.stringify(before?.map((message) => message.content))
  );

  // snapshot_required without a loader reports the recovery payload; the
  // durable protocol remains the only session protocol.
  assert.equal(apply('thread_snapshot_required', { event: 'thread_snapshot_required', data: {
    required_from_seq: 6, earliest_available_seq: 9
  } }), true);
  assert.equal(snapshotFallbacks, 1);
  assert.equal(isChatThreadV2Session(SESSION), true);
  assert.equal(apply('thread_change', wireChange({ change_type: 'item_upsert', cursor: 3 })), true);
});

test('frame normalization accepts nested emit-path, flat feeder and target-contract shapes', () => {
  // Legacy emit-path ack still maps change_cursor to resume_from_seq; content
  // is never surfaced from the ack.
  const ack = toChatThreadFrame('stream_started', {
    event: 'stream_started',
    data: { session_id: SESSION, timestamp: 't', data: { turn_id: 'turn-9', change_cursor: 4, content: 'q' } }
  });
  assert.ok(ack && ack.event === 'stream_started');
  if (ack && ack.event === 'stream_started') {
    assert.equal(ack.turn_id, 'turn-9');
    assert.equal(ack.resume_from_seq, 4);
    assert.equal((ack as Record<string, unknown>).content, undefined);
  }

  const flat = toChatThreadFrame('thread_change', {
    event: 'thread_change',
    data: { change_type: 'turn_upsert', turn_id: 'turn-9', cursor: 9, payload: { status: 'running' } }
  });
  assert.ok(flat && flat.event === 'thread_change');
  if (flat && flat.event === 'thread_change') {
    assert.equal(flat.seq, 9);
    assert.equal((flat.data as Record<string, unknown>).status, 'running');
  }

  // Target contract: thread_change.data.payload is the full immutable item
  // row (columns + embedded payload copy) for item_upsert.
  const row = toChatThreadFrame('thread_change', {
    event: 'thread_change',
    data: {
      change_type: 'item_upsert', turn_id: 'turn-9', item_id: 'turn-9:text-1',
      revision: 2, cursor: 10,
      payload: {
        item_id: 'turn-9:text-1', turn_id: 'turn-9', kind: 'assistant_message',
        status: 'running', revision: 2, visibility: 'user',
        payload: { role: 'assistant', model_round: 1, user_round: 1, content: 'target-shape' }
      }
    }
  });
  assert.ok(row && row.event === 'thread_change');
  if (row && row.event === 'thread_change') {
    assert.equal(row.seq, 10);
    assert.equal(row.revision, 2);
    assert.equal((row.data as Record<string, unknown>).content, 'target-shape');
    assert.equal((row.data as Record<string, unknown>).model_round, 1);
    assert.equal((row.data as Record<string, unknown>).kind, 'assistant_message');
  }

  // snapshot_required passes its recovery data through.
  const required = toChatThreadFrame('thread_snapshot_required', {
    event: 'thread_snapshot_required',
    data: { required_from_seq: 600, earliest_available_seq: 900 }
  });
  assert.ok(required && required.event === 'thread_snapshot_required');
  if (required && required.event === 'thread_snapshot_required') {
    assert.equal(required.required_from_seq, 600);
    assert.equal(required.earliest_available_seq, 900);
  }

  // stream_overflow is recognized with its control data.
  const overflow = toChatThreadFrame('stream_overflow', {
    event: 'stream_overflow',
    data: { cursor: 1234, resume_recommended: true }
  });
  assert.ok(overflow && overflow.event === 'stream_overflow');
  if (overflow && overflow.event === 'stream_overflow') {
    assert.equal(overflow.cursor, 1234);
    assert.equal(overflow.resume_recommended, true);
  }

  // Admin items are embedded but the projection must exclude them.
  assert.equal(toChatThreadFrame('thread_change', {
    event: 'thread_change',
    data: { change_type: 'item_upsert', cursor: 0 }
  }), null, 'cursor-less frames are dropped');

  const tail = toChatThreadFrame('thread_item_tail', {
    event: 'thread_item_tail',
    data: { item_id: 'turn-9:text-1', field: 'reasoning', offset: 3, base_seq: 12, text: '思考' }
  });
  assert.ok(tail && tail.event === 'thread_item_tail');
  if (tail && tail.event === 'thread_item_tail') {
    assert.equal(tail.field, 'reasoning');
    assert.equal(tail.offset, 3);
    assert.equal(tail.base_seq, 12);
  }
});

test('state defaults stay isolated when the feature gate is consulted', () => {
  const state = emptyChatThreadState('sess-x');
  assert.equal(state.lastSeq, 0);
  assert.equal(state.items.size, 0);
  resetChatThreadRuntime('sess-x');
  assert.equal(getChatThreadState('sess-x'), null);
  markChatThreadFallback('sess-x', 'noop');
});

test('tail frames ahead of the replay cursor are dropped until their base seq lands (I3)', () => {
  resetChatThreadRuntime(SESSION);
  ensureChatThreadRuntime(SESSION);
  const store = new FakeStore();
  const apply = (type: string, payload: unknown) =>
    applyChatThreadServerEvent(store, SESSION, type, payload);

  apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-3', cursor: 1, revision: 1,
    item: {
      item_id: 'turn-3:text-1', turn_id: 'turn-3', kind: 'assistant_message',
      status: 'running', revision: 1, visibility: 'user',
      payload: { model_round: 1, role: 'assistant', content: '', reasoning: '' }
    }
  }));
  // base_seq 5 is ahead of lastSeq 1: dropped, the bubble stays empty.
  assert.equal(apply('thread_item_tail', wireTail({
    item_id: 'turn-3:text-1', field: 'content', offset: 0, base_seq: 5, text: 'early'
  })), true);
  let messages = buildChatThreadMaterializedMessages(SESSION);
  assert.equal((messages?.[0] as Record<string, unknown> | undefined)?.content ?? '', '');

  // The durable frames up to the tail's base seq land; the tail now applies.
  assert.equal(apply('thread_change', wireChange({
    change_type: 'turn_status', turn_id: 'turn-3', cursor: 2,
    payload: { turn_id: 'turn-3', status: 'running' }
  })), true);
  assert.equal(apply('thread_change', wireChange({
    change_type: 'text_block', turn_id: 'turn-3', item_id: 'turn-3:text-1', cursor: 3,
    payload: { item_id: 'turn-3:text-1', field: 'content', block_index: 0, content_offset: 0, content: 'fixed' }
  })), true);
  assert.equal(apply('thread_item_tail', wireTail({
    item_id: 'turn-3:text-1', field: 'content', offset: 5, base_seq: 3, text: '+tail'
  })), true);
  messages = buildChatThreadMaterializedMessages(SESSION);
  assert.equal((messages?.[0] as Record<string, unknown> | undefined)?.content, 'fixed+tail');
});

test('stream_overflow resumes once per cooldown window and shares it with gap overflow', () => {
  resetChatThreadRuntime(SESSION);
  ensureChatThreadRuntime(SESSION);
  const store = new FakeStore();
  let clock = 10000;
  let overflows = 0;
  let gapOverflows = 0;
  const apply = (type: string, payload: unknown) =>
    applyChatThreadServerEvent(store, SESSION, type, payload, {
      now: () => clock,
      onOverflow: () => { overflows += 1; },
      onGapOverflow: () => { gapOverflows += 1; }
    });
  const overflow = (cursor: number) =>
    apply('stream_overflow', { event: 'stream_overflow', data: { cursor, resume_recommended: true } });

  overflow(9);
  assert.equal(overflows, 1);
  // Duplicates inside the cooldown window are suppressed.
  clock += 400;
  overflow(9);
  clock += 300;
  overflow(9);
  assert.equal(overflows, 1);
  // Past the cooldown the recovery fires again.
  clock += 500;
  overflow(9);
  assert.equal(overflows, 2);

  // Gap overflow shares the same per-session resume cooldown: overflow the
  // gap buffer (64 frames) right after the overflow dispatch — suppressed.
  clock += 10;
  for (let seq = 101; seq <= 101 + THREAD_GAP_MAX_FRAMES; seq += 1) {
    apply('thread_change', wireChange({ change_type: 'turn_status', turn_id: 't-g', cursor: seq }));
  }
  assert.equal(gapOverflows, 0, 'gap overflow suppressed inside the overflow cooldown');
  // Past the cooldown, an aged gap frame triggers the gap resume.
  clock += THREAD_OVERFLOW_RESUME_COOLDOWN_MS + 1;
  apply('thread_change', wireChange({ change_type: 'turn_status', turn_id: 't-g', cursor: 300 }));
  clock += THREAD_GAP_MAX_MS + 1;
  apply('thread_change', wireChange({ change_type: 'turn_status', turn_id: 't-g', cursor: 301 }));
  assert.equal(gapOverflows, 1);
});

test('snapshot_required rebuilds state from the injected atomic snapshot loader', async () => {
  resetChatThreadRuntime(SESSION);
  ensureChatThreadRuntime(SESSION);
  const store = new FakeStore();
  let appliedSnapshots = 0;
  let legacyFallbacks = 0;
  registerChatThreadSnapshotLoader(async (sessionKey) => {
    assert.equal(sessionKey, SESSION);
    return {
      cursor: 20,
      turns: [{ turn_id: 'turn-s', user_round: 1, status: 'running', client_message_id: 'cm-s' }],
      items: [
        // Atomic snapshot rows carry the embedded payload copy.
        {
          item_id: 'turn-s:text-1', turn_id: 'turn-s', kind: 'assistant_message',
          status: 'running', revision: 1, visibility: 'user',
          payload: { model_round: 1, role: 'assistant', content: '', reasoning: '' }
        }
      ],
      blocks: [{ item_id: 'turn-s:text-1', field: 'content', block_index: 0, content_offset: 0, content: '快照文本 ' }]
    };
  });
  const apply = (type: string, payload: unknown) =>
    applyChatThreadServerEvent(store, SESSION, type, payload, {
      onSnapshotApplied: () => { appliedSnapshots += 1; },
      onSnapshotRequired: () => { legacyFallbacks += 1; }
    });

  // Pre-seed progress the snapshot must supersede.
  apply('thread_change', wireChange({
    change_type: 'turn_upsert', turn_id: 'turn-x', cursor: 5,
    payload: { turn_id: 'turn-x', status: 'running', user_round: 1 }
  }));
  apply('thread_snapshot_required', { event: 'thread_snapshot_required', data: {
    required_from_seq: 6, earliest_available_seq: 9
  } });
  // The session stays v2 while the atomic rebuild is in flight.
  assert.equal(isChatThreadV2Session(SESSION), true);
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(appliedSnapshots, 1);
  assert.equal(legacyFallbacks, 0);
  const state = getChatThreadState(SESSION);
  assert.equal(state?.lastSeq, 20);
  assert.equal(state?.turns.has('turn-x'), false);
  assert.equal(state?.turns.get('turn-s')?.clientMessageId, 'cm-s');

  // Replay continues from the snapshot cursor.
  assert.equal(apply('thread_change', wireChange({
    change_type: 'text_block', turn_id: 'turn-s', item_id: 'turn-s:text-1', cursor: 21,
    payload: { item_id: 'turn-s:text-1', field: 'content', block_index: 1, content_offset: 5, content: '续传' }
  })), true);
  const messages = buildChatThreadMaterializedMessages(SESSION);
  assert.equal((messages?.[0] as Record<string, unknown> | undefined)?.content, '快照文本 续传');

  registerChatThreadSnapshotLoader(null);
});

test('a failed or stale snapshot keeps the durable protocol contract', async () => {
  resetChatThreadRuntime(SESSION);
  ensureChatThreadRuntime(SESSION);
  const store = new FakeStore();
  let appliedSnapshots = 0;
  let legacyFallbacks = 0;
  registerChatThreadSnapshotLoader(async () => {
    throw new Error('snapshot unavailable');
  });
  const apply = (type: string, payload: unknown) =>
    applyChatThreadServerEvent(store, SESSION, type, payload, {
      onSnapshotApplied: () => { appliedSnapshots += 1; },
      onSnapshotRequired: () => { legacyFallbacks += 1; }
    });

  apply('thread_change', wireChange({
    change_type: 'turn_upsert', turn_id: 'turn-y', cursor: 2,
    payload: { turn_id: 'turn-y', status: 'running' }
  }));
  apply('thread_snapshot_required', { event: 'thread_snapshot_required', data: {} });
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(appliedSnapshots, 0);
  assert.equal(legacyFallbacks, 1);
  assert.equal(isChatThreadV2Session(SESSION), true);

  registerChatThreadSnapshotLoader(null);
});

test('out-of-order durable frames heal through the bounded gap buffer in seq order', () => {
  resetChatThreadRuntime(SESSION);
  ensureChatThreadRuntime(SESSION);
  const store = new FakeStore();
  const apply = (type: string, payload: unknown) =>
    applyChatThreadServerEvent(store, SESSION, type, payload);

  apply('thread_change', wireChange({
    change_type: 'turn_upsert', turn_id: 'turn-g', cursor: 1, revision: 1,
    payload: { turn_id: 'turn-g', status: 'running', user_round: 1 }
  }));
  apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-g', cursor: 2, revision: 1,
    item: {
      item_id: 'turn-g:text-1', turn_id: 'turn-g', kind: 'assistant_message',
      status: 'running', revision: 1, visibility: 'user',
      payload: { model_round: 1, role: 'assistant', content: '', reasoning: '' }
    }
  }));

  // Far-ahead frames wait in the gap buffer; lastSeq must not advance.
  apply('thread_change', wireChange({
    change_type: 'turn_status', turn_id: 'turn-g', cursor: 5,
    payload: { turn_id: 'turn-g', status: 'e' }
  }));
  apply('thread_change', wireChange({
    change_type: 'turn_status', turn_id: 'turn-g', cursor: 4,
    payload: { turn_id: 'turn-g', status: 'd' }
  }));
  const stateBeforeHeal = getChatThreadState(SESSION)!;
  assert.equal(stateBeforeHeal.lastSeq, 2);
  assert.equal(stateBeforeHeal.gap.length, 2);

  // The missing head lands: everything buffers forward in commit order.
  apply('thread_change', wireChange({
    change_type: 'turn_status', turn_id: 'turn-g', cursor: 3,
    payload: { turn_id: 'turn-g', status: 'c' }
  }));
  const stateAfterHeal = getChatThreadState(SESSION)!;
  assert.equal(stateAfterHeal.lastSeq, 5);
  assert.equal(stateAfterHeal.gap.length, 0);
  assert.equal(getThreadTurn(stateAfterHeal, 'turn-g')?.status, 'e');
});

test('revision rollback and same-revision replay never regress item state', () => {
  resetChatThreadRuntime(SESSION);
  ensureChatThreadRuntime(SESSION);
  const store = new FakeStore();
  const apply = (type: string, payload: unknown) =>
    applyChatThreadServerEvent(store, SESSION, type, payload);

  apply('thread_change', wireChange({
    change_type: 'turn_upsert', turn_id: 'turn-r', cursor: 1, revision: 1,
    payload: { turn_id: 'turn-r', status: 'running', user_round: 1 }
  }));
  apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-r', cursor: 2, revision: 1,
    item: {
      item_id: 'turn-r:text-1', turn_id: 'turn-r', kind: 'assistant_message',
      status: 'running', revision: 1, visibility: 'user',
      payload: { model_round: 1, role: 'assistant', content: '一', reasoning: '' }
    }
  }));
  // Higher seq but lower revision: stale rollback payload — ignored.
  apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-r', cursor: 2, revision: 0,
    item: {
      item_id: 'turn-r:text-1', turn_id: 'turn-r', kind: 'assistant_message',
      status: 'running', revision: 0, visibility: 'user',
      payload: { model_round: 1, role: 'assistant', content: '回退', reasoning: '' }
    }
  }));
  assert.equal(getChatThreadState(SESSION)?.items.get('turn-r:text-1')?.revision, 1);

  // Same revision replay (delivery retry): idempotent.
  apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-r', cursor: 2, revision: 1,
    item: {
      item_id: 'turn-r:text-1', turn_id: 'turn-r', kind: 'assistant_message',
      status: 'running', revision: 1, visibility: 'user',
      payload: { model_round: 1, role: 'assistant', content: '一-改', reasoning: '' }
    }
  }));
  assert.equal(
    getChatThreadState(SESSION)?.items.get('turn-r:text-1')?.content,
    '一',
    'same-revision replay must not mutate content'
  );

  // Forward revision applies.
  apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-r', cursor: 3, revision: 2,
    item: {
      item_id: 'turn-r:text-1', turn_id: 'turn-r', kind: 'assistant_message',
      status: 'completed', revision: 2, visibility: 'user',
      payload: { model_round: 1, role: 'assistant', content: '二', reasoning: '' }
    }
  }));
  const messages = buildChatThreadMaterializedMessages(SESSION);
  assert.equal((messages?.[0] as Record<string, unknown> | undefined)?.content, '二');
});

test('tail frames validate UTF-16 offsets: zero, continuous, hole, covered and -1', () => {
  resetChatThreadRuntime(SESSION);
  ensureChatThreadRuntime(SESSION);
  const store = new FakeStore();
  const apply = (type: string, payload: unknown) =>
    applyChatThreadServerEvent(store, SESSION, type, payload);
  const state = () => getChatThreadState(SESSION)!;

  apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-t', cursor: 1, revision: 1,
    item: {
      item_id: 'turn-t:text-1', turn_id: 'turn-t', kind: 'assistant_message',
      status: 'running', revision: 1, visibility: 'user',
      payload: { model_round: 1, role: 'assistant', content: '', reasoning: '' }
    }
  }));

  // offset 0 at the durable block-end watermark 0.
  apply('thread_item_tail', wireTail({
    item_id: 'turn-t:text-1', field: 'content', offset: 0, text: '你好'
  }));
  assert.equal(composeItemText(state(), 'turn-t:text-1', 'content'), '你好');

  // Continuous offset appends (你好=2 code units).
  apply('thread_item_tail', wireTail({
    item_id: 'turn-t:text-1', field: 'content', offset: 2, text: '，世界'
  }));
  assert.equal(composeItemText(state(), 'turn-t:text-1', 'content'), '你好，世界');

  // UTF-16 code-unit counting: 😀 occupies two code units, offset 5 is exact.
  apply('thread_item_tail', wireTail({
    item_id: 'turn-t:text-1', field: 'content', offset: 5, text: '😀'
  }));
  assert.equal(composeItemText(state(), 'turn-t:text-1', 'content'), '你好，世界😀');

  // Hole: offset ahead of the tail position is dropped until a block heals it.
  apply('thread_item_tail', wireTail({
    item_id: 'turn-t:text-1', field: 'content', offset: 12, text: '未来'
  }));
  assert.equal(composeItemText(state(), 'turn-t:text-1', 'content'), '你好，世界😀');

  // Covered: offset behind the tail position is dropped as a replay.
  apply('thread_item_tail', wireTail({
    item_id: 'turn-t:text-1', field: 'content', offset: 3, text: '重复'
  }));
  assert.equal(composeItemText(state(), 'turn-t:text-1', 'content'), '你好，世界😀');

  // offset -1 is arrival-order append (tool/command output without durable
  // offsets) and builds on the field tail in arrival order.
  apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-t', cursor: 2, revision: 1,
    item: {
      item_id: 'turn-t:tool-1', turn_id: 'turn-t', kind: 'tool_call',
      status: 'running', revision: 1, visibility: 'user',
      payload: { model_round: 1, tool_call_id: 'call_t', name: 'tool' }
    }
  }));
  apply('thread_item_tail', wireTail({
    item_id: 'turn-t:tool-1', field: 'output', offset: -1, text: 'a'
  }));
  apply('thread_item_tail', wireTail({
    item_id: 'turn-t:tool-1', field: 'output', offset: -1, text: 'b'
  }));
  assert.equal(composeItemText(state(), 'turn-t:tool-1', 'output'), 'ab');
  // A durable block heals the -1 tail for its own field only.
  apply('thread_change', wireChange({
    change_type: 'text_block', turn_id: 'turn-t', item_id: 'turn-t:tool-1', cursor: 3,
    payload: { item_id: 'turn-t:tool-1', field: 'output', block_index: 0, content_offset: 0, content: 'durable-output' }
  }));
  assert.equal(composeItemText(state(), 'turn-t:tool-1', 'output'), 'durable-output');
  assert.equal(composeItemText(state(), 'turn-t:text-1', 'content'), '你好，世界😀');
});

test('tails stay isolated per (item_id, field) across interleaved model rounds', () => {
  resetChatThreadRuntime(SESSION);
  ensureChatThreadRuntime(SESSION);
  const store = new FakeStore();
  const apply = (type: string, payload: unknown) =>
    applyChatThreadServerEvent(store, SESSION, type, payload);

  apply('thread_change', wireChange({
    change_type: 'turn_upsert', turn_id: 'turn-i', cursor: 1, revision: 1,
    payload: { turn_id: 'turn-i', status: 'running', user_round: 1 }
  }));
  apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-i', cursor: 2, revision: 1,
    item: {
      item_id: 'turn-i:text-1', turn_id: 'turn-i', kind: 'assistant_message',
      status: 'running', revision: 1, visibility: 'user',
      payload: { model_round: 1, role: 'assistant', content: '', reasoning: '' }
    }
  }));
  apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-i', cursor: 3, revision: 1,
    item: {
      item_id: 'turn-i:text-2', turn_id: 'turn-i', kind: 'assistant_message',
      status: 'running', revision: 1, visibility: 'user',
      payload: { model_round: 2, role: 'assistant', content: '', reasoning: '' }
    }
  }));

  // Interleaved tails for round 2 and round 1 never cross.
  apply('thread_item_tail', wireTail({ item_id: 'turn-i:text-2', field: 'content', offset: 0, text: '世界' }));
  apply('thread_item_tail', wireTail({ item_id: 'turn-i:text-1', field: 'content', offset: 0, text: '你好' }));
  apply('thread_item_tail', wireTail({ item_id: 'turn-i:text-2', field: 'content', offset: 2, text: '！' }));
  apply('thread_item_tail', wireTail({ item_id: 'turn-i:text-1', field: 'content', offset: 2, text: '，我是' }));
  apply('thread_item_tail', wireTail({ item_id: 'turn-i:text-1', field: 'reasoning', offset: 0, text: '思考' }));

  const messages = buildChatThreadMaterializedMessages(SESSION);
  assert.deepEqual(
    messages?.map((message) => message.content),
    ['你好，我是', '世界！']
  );
  assert.equal(
    (messages?.[0] as Record<string, unknown> | undefined)?.reasoning,
    '思考'
  );
});

test('a stale atomic snapshot is refused and keeps the durable protocol contract', async () => {
  resetChatThreadRuntime(SESSION);
  ensureChatThreadRuntime(SESSION);
  const store = new FakeStore();
  let appliedSnapshots = 0;
  let legacyFallbacks = 0;
  registerChatThreadSnapshotLoader(async () => ({
    cursor: 1,
    turns: [{ turn_id: 'turn-stale', user_round: 1, status: 'running' }],
    items: [],
    blocks: []
  }));
  const apply = (type: string, payload: unknown) =>
    applyChatThreadServerEvent(store, SESSION, type, payload, {
      onSnapshotApplied: () => { appliedSnapshots += 1; },
      onSnapshotRequired: () => { legacyFallbacks += 1; }
    });

  apply('thread_change', wireChange({
    change_type: 'turn_upsert', turn_id: 'turn-live', cursor: 1, revision: 1,
    payload: { turn_id: 'turn-live', status: 'running', user_round: 1 }
  }));
  apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-live', cursor: 2, revision: 1,
    item: {
      item_id: 'turn-live:text-1', turn_id: 'turn-live', kind: 'assistant_message',
      status: 'running', revision: 1, visibility: 'user',
      payload: { model_round: 1, role: 'assistant', content: '存活', reasoning: '' }
    }
  }));
  assert.equal(getChatThreadState(SESSION)?.lastSeq, 2);

  apply('thread_snapshot_required', { event: 'thread_snapshot_required', data: {
    required_from_seq: 3, earliest_available_seq: 4
  } });
  await new Promise((resolve) => setTimeout(resolve, 0));

  // The atomic snapshot cursor (1) is behind the local durable cursor (2):
  // refusing it must not overwrite newer state.
  assert.equal(appliedSnapshots, 0);
  assert.equal(legacyFallbacks, 1);
  assert.equal(isChatThreadV2Session(SESSION), true);
  assert.equal(getChatThreadState(SESSION)?.lastSeq, 2);
  // A stale snapshot leaves the durable projection intact.
  assert.equal(buildChatThreadMaterializedMessages(SESSION)?.length, 1);

  registerChatThreadSnapshotLoader(null);
});

test('gap overflow at the frame limit triggers one resume and clears the buffer', () => {
  resetChatThreadRuntime(SESSION);
  ensureChatThreadRuntime(SESSION);
  const store = new FakeStore();
  let clock = 8000;
  let gapOverflows = 0;
  const apply = (type: string, payload: unknown) =>
    applyChatThreadServerEvent(store, SESSION, type, payload, {
      now: () => clock,
      onGapOverflow: () => { gapOverflows += 1; }
    });

  apply('thread_change', wireChange({
    change_type: 'turn_upsert', turn_id: 'turn-q', cursor: 1, revision: 1,
    payload: { turn_id: 'turn-q', status: 'running' }
  }));

  // Exactly the frame limit stays buffered without a resume.
  for (let seq = 100; seq < 100 + THREAD_GAP_MAX_FRAMES; seq += 1) {
    apply('thread_change', wireChange({
      change_type: 'turn_status', turn_id: 'turn-q', cursor: seq,
      payload: { turn_id: 'turn-q', status: 'waiting' }
    }));
  }
  assert.equal(gapOverflows, 0);
  assert.equal(getChatThreadState(SESSION)?.gap.length, THREAD_GAP_MAX_FRAMES);

  // One frame past the limit overflows: single-flight resume, buffer cleared,
  // durable cursor untouched (the resume replays from lastSeq).
  apply('thread_change', wireChange({
    change_type: 'turn_status', turn_id: 'turn-q', cursor: 100 + THREAD_GAP_MAX_FRAMES,
    payload: { turn_id: 'turn-q', status: 'waiting' }
  }));
  assert.equal(gapOverflows, 1);
  assert.equal(getChatThreadState(SESSION)?.gap.length, 0);
  assert.equal(getChatThreadState(SESSION)?.lastSeq, 1);

  // A burst past the limit collapses into the same single resume dispatch
  // (overflow and gap overflow share one per-session cooldown).
  for (let seq = 200; seq < 200 + THREAD_GAP_MAX_FRAMES + 4; seq += 1) {
    apply('thread_change', wireChange({
      change_type: 'turn_status', turn_id: 'turn-q', cursor: seq,
      payload: { turn_id: 'turn-q', status: 'waiting' }
    }));
  }
  assert.equal(gapOverflows, 1, 'burst inside the cooldown window is suppressed');

  // Past the cooldown, a fresh overflow dispatches a new resume.
  clock += THREAD_OVERFLOW_RESUME_COOLDOWN_MS + 1;
  for (let seq = 300; seq < 300 + THREAD_GAP_MAX_FRAMES + 1; seq += 1) {
    apply('thread_change', wireChange({
      change_type: 'turn_status', turn_id: 'turn-q', cursor: seq,
      payload: { turn_id: 'turn-q', status: 'waiting' }
    }));
  }
  assert.equal(gapOverflows, 2);
  assert.equal(getChatThreadState(SESSION)?.lastSeq, 1);
  assert.ok(
    (getChatThreadState(SESSION)?.gap.length ?? 0) <= THREAD_GAP_MAX_FRAMES,
    'buffer stays bounded after the overflow dispatch'
  );
});

test('duplicate stream_started acks bind the optimistic turn exactly once', () => {
  resetChatThreadRuntime(SESSION);
  ensureChatThreadRuntime(SESSION);
  const store = new FakeStore();
  const apply = (type: string, payload: unknown) =>
    applyChatThreadServerEvent(store, SESSION, type, payload);

  apply('stream_started', wireAck({
    turn_id: 'turn-d', user_round: 1, resume_from_seq: 2,
    client_message_id: 'cm-d'
  }));
  apply('stream_started', wireAck({
    turn_id: 'turn-d', user_round: 1, resume_from_seq: 2,
    client_message_id: 'cm-d'
  }));
  assert.equal(getChatThreadState(SESSION)?.turns.size, 1);
  assert.equal(getThreadTurn(getChatThreadState(SESSION)!, 'turn-d')?.clientMessageId, 'cm-d');
});

test('long tail streaming reuses materialized rows without rebuilding history', () => {
  resetChatThreadRuntime(SESSION);
  ensureChatThreadRuntime(SESSION);
  const store = new FakeStore();
  const apply = (type: string, payload: unknown) =>
    applyChatThreadServerEvent(store, SESSION, type, payload);

  // Three settled turns plus one live turn (the 1000-token-class scenario).
  for (let round = 1; round <= 3; round += 1) {
    apply('thread_change', wireChange({
      change_type: 'turn_upsert', turn_id: `turn-p${round}`, cursor: round * 2 - 1, revision: 1,
      payload: { turn_id: `turn-p${round}`, status: 'completed', user_round: round }
    }));
    apply('thread_change', wireChange({
      change_type: 'item_upsert', turn_id: `turn-p${round}`, cursor: round * 2, revision: 1,
      item: {
        item_id: `turn-p${round}:text-1`, turn_id: `turn-p${round}`, kind: 'assistant_message',
        status: 'completed', revision: 1, visibility: 'user',
        payload: { model_round: 1, role: 'assistant', content: `历史${round}`, reasoning: '' }
      }
    }));
  }
  apply('thread_change', wireChange({
    change_type: 'turn_upsert', turn_id: 'turn-live', cursor: 7, revision: 1,
    payload: { turn_id: 'turn-live', status: 'running', user_round: 4 }
  }));
  apply('thread_change', wireChange({
    change_type: 'item_upsert', turn_id: 'turn-live', cursor: 8, revision: 1,
    item: {
      item_id: 'turn-live:text-1', turn_id: 'turn-live', kind: 'assistant_message',
      status: 'running', revision: 1, visibility: 'user',
      payload: { model_round: 1, role: 'assistant', content: '', reasoning: '' }
    }
  }));

  // 1000 code units streamed as low-latency tails with continuous offsets.
  let offset = 0;
  for (let index = 0; index < 500; index += 1) {
    apply('thread_item_tail', wireTail({
      item_id: 'turn-live:text-1', field: 'content', offset, text: '你好'
    }));
    offset += 2;
  }
  const first = buildChatThreadMaterializedMessages(SESSION);
  assert.equal(first?.length, 4);

  // Another 500 tails: history rows and the active row keep their object
  // identity — no per-token rebuild of the materialized history.
  for (let index = 0; index < 500; index += 1) {
    apply('thread_item_tail', wireTail({
      item_id: 'turn-live:text-1', field: 'content', offset, text: '啊'
    }));
    offset += 1;
  }
  const second = buildChatThreadMaterializedMessages(SESSION);
  assert.equal(second?.length, 4);
  assert.equal(second?.[0], first?.[0], 'history row is reused');
  assert.equal(second?.[1], first?.[1], 'history row is reused');
  assert.equal(second?.[2], first?.[2], 'history row is reused');
  assert.equal(second?.[3], first?.[3], 'active row is reused');
  assert.equal(String(second?.[3]?.content).length, 1500);
  assert.equal(String(second?.[0]?.content), '历史1');
});
