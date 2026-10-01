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
import { getThreadTurn } from '../../src/realtime/chat/chatThreadState';
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

test('v2 pipeline falls back to legacy when snapshot recovery is unavailable, and idempotency holds', () => {
  resetChatThreadRuntime(SESSION);
  ensureChatThreadRuntime(SESSION);
  registerChatThreadSnapshotLoader(null); // no loader wired: legacy fallback contract
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

  // snapshot_required without a loader flips the session back to the legacy
  // pipeline and reports the recovery payload.
  assert.equal(apply('thread_snapshot_required', { event: 'thread_snapshot_required', data: {
    required_from_seq: 6, earliest_available_seq: 9
  } }), true);
  assert.equal(snapshotFallbacks, 1);
  assert.equal(isChatThreadV2Session(SESSION), false);
  assert.equal(apply('thread_change', wireChange({ change_type: 'item_upsert', cursor: 3 })), false);
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

test('a failed or stale snapshot keeps the legacy fallback contract', async () => {
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
  assert.equal(isChatThreadV2Session(SESSION), false);

  registerChatThreadSnapshotLoader(null);
});
