import test from 'node:test';
import assert from 'node:assert/strict';

import {
  applyChatThreadFrame,
  applyChatThreadSnapshot,
  bindStreamStarted,
  composeItemText
} from '../../src/realtime/chat/chatThreadState';
import type { ChatThreadSnapshot } from '../../src/realtime/chat/chatThreadState';
import {
  emptyChatThreadState,
  THREAD_GAP_MAX_FRAMES
} from '../../src/realtime/chat/chatThreadTypes';
import type {
  ChatThreadApplyResult,
  ChatThreadFrame,
  StreamStartedAck,
  ThreadChangeFrame,
  ThreadItemTailFrame
} from '../../src/realtime/chat/chatThreadTypes';

const change = (seq: number, changeType: string, data: Record<string, unknown>): ThreadChangeFrame => ({
  event: 'thread_change',
  seq,
  change_type: changeType,
  data
});

const tail = (
  itemId: string,
  field: string,
  offset: number,
  text: string,
  baseSeq?: number
): ThreadItemTailFrame => ({
  event: 'thread_item_tail',
  item_id: itemId,
  field,
  offset,
  ...(baseSeq === undefined ? {} : { base_seq: baseSeq }),
  text
});

const upsertItem = (seq: number, data: Record<string, unknown>): ThreadChangeFrame =>
  change(seq, 'item_upsert', { turn_id: 't1', kind: 'assistant_message', revision: 1, ...data });

test('duplicate seq frames are idempotent', () => {
  const state = emptyChatThreadState('session-1');
  const frame = change(1, 'turn_upsert', {
    turn_id: 't1',
    user_round: 1,
    status: 'running',
    content: 'question',
    client_message_id: 'cm-1'
  });

  const first = applyChatThreadFrame(state, frame, 0);
  assert.equal(first.changed, true);
  const second = applyChatThreadFrame(state, frame, 1);
  assert.deepEqual(
    { changed: second.changed, structural: second.structural, needResume: second.needResume, droppedEphemeral: second.droppedEphemeral },
    { changed: false, structural: false, needResume: false, droppedEphemeral: 0 }
  );
  assert.equal(state.lastSeq, 1);
  assert.equal(state.turns.get('t1')?.userContent, 'question');
});

test('out-of-order durable frames drain the gap in seq order', () => {
  const state = emptyChatThreadState('session-1');
  applyChatThreadFrame(state, change(1, 'turn_upsert', { turn_id: 't1', status: 'running' }), 0);

  const buffered = applyChatThreadFrame(
    state,
    upsertItem(3, { item_id: 'i3', content: 'third' }),
    0
  );
  assert.equal(buffered.changed, false);
  assert.equal(state.lastSeq, 1);
  assert.equal(state.items.has('i3'), false);

  const unstuck = applyChatThreadFrame(
    state,
    upsertItem(2, { item_id: 'i2', content: 'second' }),
    0
  );
  assert.equal(unstuck.changed, true);
  assert.equal(unstuck.structural, true);
  assert.equal(state.lastSeq, 3);
  assert.equal(state.items.get('i2')?.content, 'second');
  assert.equal(state.items.get('i3')?.content, 'third');
  assert.equal(state.gap.length, 0);
});

test(`gap buffer overflow above ${THREAD_GAP_MAX_FRAMES} frames triggers resume`, () => {
  const state = emptyChatThreadState('session-1');
  applyChatThreadFrame(state, change(1, 'turn_upsert', { turn_id: 't1' }), 0);

  let last: ChatThreadApplyResult = applyChatThreadFrame(
    state,
    change(3, 'turn_status', { turn_id: 't1', status: 'running' }),
    0
  );
  for (let seq = 4; seq <= 2 + THREAD_GAP_MAX_FRAMES; seq += 1) {
    last = applyChatThreadFrame(state, change(seq, 'turn_status', { turn_id: 't1', status: 'running' }), 0);
    assert.equal(last.needResume, false);
  }
  assert.equal(state.gap.length, THREAD_GAP_MAX_FRAMES);

  last = applyChatThreadFrame(
    state,
    change(3 + THREAD_GAP_MAX_FRAMES, 'turn_status', { turn_id: 't1', status: 'running' }),
    0
  );
  assert.equal(last.needResume, true);
  assert.equal(last.changed, false);
  assert.equal(state.gap.length, 0);
  assert.equal(state.lastSeq, 1);
});

test('aged gap frames trigger resume after the time bound', () => {
  const state = emptyChatThreadState('session-1');
  applyChatThreadFrame(state, change(1, 'turn_upsert', { turn_id: 't1' }), 1000);

  const buffered = applyChatThreadFrame(
    state,
    change(3, 'turn_status', { turn_id: 't1', status: 'running' }),
    1000
  );
  assert.equal(buffered.needResume, false);

  const aged = applyChatThreadFrame(
    state,
    change(5, 'turn_status', { turn_id: 't1', status: 'running' }),
    1000 + 1001
  );
  assert.equal(aged.needResume, true);
  assert.equal(state.gap.length, 0);
  assert.equal(state.lastSeq, 1);
});

test('item upserts accept only higher revisions', () => {
  const state = emptyChatThreadState('session-1');
  applyChatThreadFrame(state, upsertItem(1, { item_id: 'i1', revision: 2, content: 'v2' }), 0);

  const stale = applyChatThreadFrame(state, upsertItem(2, { item_id: 'i1', revision: 1, content: 'v1' }), 0);
  assert.equal(stale.changed, false);
  assert.equal(state.items.get('i1')?.content, 'v2');
  assert.equal(state.items.get('i1')?.revision, 2);

  const same = applyChatThreadFrame(state, upsertItem(3, { item_id: 'i1', revision: 2, content: 'v2-again' }), 0);
  assert.equal(same.changed, false);
  assert.equal(state.items.get('i1')?.content, 'v2');

  const newer = applyChatThreadFrame(state, upsertItem(4, { item_id: 'i1', revision: 3, content: 'v3', status: 'final' }), 0);
  assert.equal(newer.changed, true);
  assert.equal(newer.structural, true); // status flip
  assert.equal(state.items.get('i1')?.content, 'v3');
  assert.equal(state.items.get('i1')?.status, 'final');
});

test('admin-visibility items are stored for the projection to exclude', () => {
  const state = emptyChatThreadState('session-1');
  applyChatThreadFrame(
    state,
    change(1, 'item_upsert', {
      item_id: 'admin-1',
      turn_id: 't1',
      kind: 'note',
      revision: 1,
      visibility: 'admin',
      content: 'internal'
    }),
    0
  );
  assert.equal(state.items.get('admin-1')?.visibility, 'admin');
  assert.equal(state.items.get('admin-1')?.content, 'internal');
});

test('item registration order is monotonic across updates', () => {
  const state = emptyChatThreadState('session-1');
  applyChatThreadFrame(state, upsertItem(1, { item_id: 'i1' }), 0);
  applyChatThreadFrame(state, upsertItem(2, { item_id: 'i2' }), 0);
  applyChatThreadFrame(state, upsertItem(3, { item_id: 'i3' }), 0);
  applyChatThreadFrame(state, upsertItem(4, { item_id: 'i1', revision: 2, content: 'updated' }), 0);

  assert.equal(state.items.get('i1')?.order, 0);
  assert.equal(state.items.get('i2')?.order, 1);
  assert.equal(state.items.get('i3')?.order, 2);
});

test('tail offsets follow the equal/less/greater/arrival branches', () => {
  const state = emptyChatThreadState('session-1');
  applyChatThreadFrame(state, upsertItem(1, { item_id: 'i1' }), 0);
  applyChatThreadFrame(
    state,
    change(2, 'text_block', { item_id: 'i1', field: 'content', block_index: 0, content_offset: 0, content: '0123456789' }),
    0
  );

  const at = applyChatThreadFrame(state, tail('i1', 'content', 10, 'AB'), 0);
  assert.equal(at.changed, true);
  assert.equal(at.droppedEphemeral, 0);

  const before = applyChatThreadFrame(state, tail('i1', 'content', 5, 'zz'), 0);
  assert.equal(before.changed, false);
  assert.equal(before.droppedEphemeral, 1);

  const beyond = applyChatThreadFrame(state, tail('i1', 'content', 99, 'zz'), 0);
  assert.equal(beyond.changed, false);
  assert.equal(beyond.droppedEphemeral, 1);

  const arrival = applyChatThreadFrame(state, tail('i1', 'content', -1, 'XY'), 0);
  assert.equal(arrival.changed, true);

  assert.equal(composeItemText(state, 'i1', 'content'), '0123456789ABXY');
});

test('tails ahead of the replay cursor are dropped until their base frame lands (I3)', () => {
  const state = emptyChatThreadState('session-1');
  applyChatThreadFrame(state, upsertItem(1, { item_id: 'i1' }), 0);
  applyChatThreadFrame(
    state,
    change(2, 'text_block', { item_id: 'i1', field: 'content', block_index: 0, content_offset: 0, content: '0123456789' }),
    0
  );
  assert.equal(state.lastSeq, 2);

  // base_seq (3) is ahead of lastSeq (2): the tail must be dropped.
  const early = applyChatThreadFrame(state, tail('i1', 'content', 10, 'AB', 3), 0);
  assert.equal(early.changed, false);
  assert.equal(early.droppedEphemeral, 1);
  assert.equal(composeItemText(state, 'i1', 'content'), '0123456789');

  // The base durable frame lands; the same tail now applies.
  applyChatThreadFrame(state, change(3, 'turn_status', { turn_id: 't1', status: 'running' }), 0);
  assert.equal(state.lastSeq, 3);
  const onBase = applyChatThreadFrame(state, tail('i1', 'content', 10, 'AB', 3), 0);
  assert.equal(onBase.changed, true);
  assert.equal(onBase.droppedEphemeral, 0);
  assert.equal(composeItemText(state, 'i1', 'content'), '0123456789AB');

  // base_seq below the cursor and an explicit 0 both apply.
  const behind = applyChatThreadFrame(state, tail('i1', 'content', 12, 'CD', 2), 0);
  assert.equal(behind.changed, true);
  const zero = applyChatThreadFrame(state, tail('i1', 'content', -1, 'E', 0), 0);
  assert.equal(zero.changed, true);
  assert.equal(composeItemText(state, 'i1', 'content'), '0123456789ABCDE');
});

test('tail initializes at the block end for a fresh field', () => {
  const state = emptyChatThreadState('session-1');
  applyChatThreadFrame(state, upsertItem(1, { item_id: 'i1' }), 0);
  applyChatThreadFrame(
    state,
    change(2, 'text_block', { item_id: 'i1', field: 'reasoning', block_index: 0, reasoning_offset: 0, reasoning: 'think' }),
    0
  );

  const reasoningTail = applyChatThreadFrame(state, tail('i1', 'reasoning', 5, 'ing'), 0);
  assert.equal(reasoningTail.changed, true);
  // A tail for the other field starts from that field's own block end (0).
  const contentTail = applyChatThreadFrame(state, tail('i1', 'content', 0, 'answer'), 0);
  assert.equal(contentTail.changed, true);

  assert.equal(composeItemText(state, 'i1', 'reasoning'), 'thinking');
  assert.equal(composeItemText(state, 'i1', 'content'), 'answer');
});

test('block snapshots overwrite the same key and heal the tail', () => {
  const state = emptyChatThreadState('session-1');
  applyChatThreadFrame(state, upsertItem(1, { item_id: 'i1' }), 0);
  applyChatThreadFrame(
    state,
    change(2, 'text_block', { item_id: 'i1', field: 'content', block_index: 0, content_offset: 0, content: 'hello' }),
    0
  );
  applyChatThreadFrame(state, tail('i1', 'content', 5, ' world'), 0);
  assert.equal(composeItemText(state, 'i1', 'content'), 'hello world');

  // The durable flush covers the streamed tail.
  const flush = applyChatThreadFrame(
    state,
    change(3, 'text_block', { item_id: 'i1', field: 'content', block_index: 1, content_offset: 5, content: ' world' }),
    0
  );
  assert.equal(flush.changed, true);
  assert.equal(composeItemText(state, 'i1', 'content'), 'hello world');

  // Same-key replay is idempotent.
  const replay = applyChatThreadFrame(
    state,
    change(3, 'text_block', { item_id: 'i1', field: 'content', block_index: 1, content_offset: 5, content: ' world' }),
    0
  );
  assert.equal(replay.changed, false);
  assert.equal(composeItemText(state, 'i1', 'content'), 'hello world');

  // The tail beyond the last block end still appends.
  applyChatThreadFrame(state, tail('i1', 'content', 11, '!'), 0);
  assert.equal(composeItemText(state, 'i1', 'content'), 'hello world!');

  // A corrected snapshot overwrites the same block key without duplication.
  applyChatThreadFrame(
    state,
    change(4, 'text_block', { item_id: 'i1', field: 'content', block_index: 1, content_offset: 5, content: ' WORLD' }),
    0
  );
  assert.equal(composeItemText(state, 'i1', 'content'), 'hello WORLD!');
});

test('composeItemText falls back to the item payload without blocks or tail', () => {
  const state = emptyChatThreadState('session-1');
  applyChatThreadFrame(
    state,
    upsertItem(1, { item_id: 'i1', content: 'full text', reasoning: 'thought chain' }),
    0
  );
  assert.equal(composeItemText(state, 'i1', 'content'), 'full text');
  assert.equal(composeItemText(state, 'i1', 'reasoning'), 'thought chain');
});

test('turn upserts, status flips and terminal items keep the turn status in sync', () => {
  const state = emptyChatThreadState('session-1');
  const upsert = applyChatThreadFrame(
    state,
    change(1, 'turn_upsert', {
      turn_id: 't1',
      user_round: 2,
      status: 'running',
      content: 'question',
      client_message_id: 'cm-1'
    }),
    0
  );
  assert.equal(upsert.structural, true);
  assert.equal(state.turns.get('t1')?.status, 'running');
  assert.equal(state.turns.get('t1')?.userContent, 'question');
  assert.equal(state.turns.get('t1')?.clientMessageId, 'cm-1');

  const terminal = applyChatThreadFrame(
    state,
    change(2, 'item_upsert', { item_id: 'term-1', turn_id: 't1', kind: 'terminal', revision: 1, status: 'completed' }),
    0
  );
  assert.equal(terminal.changed, true);
  assert.equal(state.turns.get('t1')?.status, 'completed');

  const flipped = applyChatThreadFrame(state, change(3, 'turn_status', { turn_id: 't1', status: 'failed' }), 0);
  assert.equal(flipped.structural, true);
  assert.equal(state.turns.get('t1')?.status, 'failed');

  const sameStatus = applyChatThreadFrame(state, change(4, 'turn_status', { turn_id: 't1', status: 'failed' }), 0);
  assert.equal(sameStatus.changed, false);
});

test('snapshot cursor guard rejects stale snapshots and applies fresh ones wholesale', () => {
  const state = emptyChatThreadState('session-1');
  applyChatThreadFrame(state, upsertItem(1, { item_id: 'live-1', content: 'live' }), 0);

  const stale = applyChatThreadSnapshot(state, { cursor: 0, turns: [], items: [], blocks: [] }, 0);
  assert.equal(stale.changed, false);
  assert.equal(state.items.get('live-1')?.content, 'live');
  assert.equal(state.lastSeq, 1);

  const fresh = applyChatThreadSnapshot(state, {
    cursor: 12,
    turns: [{ turn_id: 't2', user_round: 3, status: 'idle', content: 'q2' }],
    items: [{ item_id: 'snap-1', turn_id: 't2', kind: 'assistant_message', revision: 4, content: 'payload only' }],
    blocks: [{ item_id: 'snap-1', field: 'content', block_index: 0, content_offset: 0, content: 'snapshot ' }]
  } satisfies ChatThreadSnapshot, 0);
  assert.equal(fresh.changed, true);
  assert.equal(fresh.structural, true);
  assert.equal(state.lastSeq, 12);
  assert.equal(state.items.has('live-1'), false);
  assert.equal(state.turns.has('t1'), false);
  assert.equal(state.items.get('snap-1')?.content, 'payload only');
  assert.equal(state.turns.get('t2')?.userContent, 'q2');
  assert.equal(state.gap.length, 0);

  // Snapshot blocks rebuild the block-end watermark, so tails resume correctly.
  applyChatThreadFrame(state, tail('snap-1', 'content', 9, 'text'), 0);
  assert.equal(composeItemText(state, 'snap-1', 'content'), 'snapshot text');

  // A same-cursor snapshot is accepted (guard is cursor < lastSeq).
  const sameCursor = applyChatThreadSnapshot(state, {
    cursor: 12,
    turns: [],
    items: [{ item_id: 'snap-2', turn_id: 't2', kind: 'assistant_message', revision: 1, content: 'x' }],
    blocks: []
  }, 0);
  assert.equal(sameCursor.changed, true);
  assert.equal(state.items.has('snap-1'), false);
  assert.equal(state.items.get('snap-2')?.content, 'x');
  assert.equal(state.items.get('snap-2')?.order, 0); // order counter restarts with the snapshot
});

test('atomic snapshot accepts full item rows with embedded payloads and empty blocks', () => {
  const state = emptyChatThreadState('session-1');
  const applied = applyChatThreadSnapshot(state, {
    cursor: 30,
    turns: [{ turn_id: 't9', user_round: 2, status: 'running', client_message_id: 'cm-9' }],
    items: [
      {
        item_id: 'i-row',
        turn_id: 't9',
        kind: 'assistant_message',
        status: 'running',
        revision: 3,
        visibility: 'user',
        payload: { role: 'assistant', model_round: 1, user_round: 2, content: '', reasoning: '' }
      }
    ],
    blocks: []
  } satisfies ChatThreadSnapshot, 0);
  assert.equal(applied.changed, true);
  assert.equal(state.lastSeq, 30);
  assert.equal(state.turns.get('t9')?.clientMessageId, 'cm-9');
  const item = state.items.get('i-row');
  assert.equal(item?.revision, 3);
  assert.equal(item?.visibility, 'user');
  assert.equal(item?.modelRound, 1);
  assert.equal(item?.content, '');

  // The rebuilt block watermark lets tails continue from the durable payload.
  applyChatThreadFrame(
    state,
    change(31, 'text_block', { item_id: 'i-row', field: 'content', block_index: 0, content_offset: 0, content: 'row text' }),
    0
  );
  applyChatThreadFrame(state, tail('i-row', 'content', 8, ' + tail', 31), 0);
  assert.equal(composeItemText(state, 'i-row', 'content'), 'row text + tail');
});

test('stream_started binds the optimistic turn by client message id or creates it', () => {
  const state = emptyChatThreadState('session-1');
  applyChatThreadFrame(
    state,
    change(1, 'turn_upsert', {
      turn_id: 'local-t1',
      user_round: 1,
      content: 'typed text',
      client_message_id: 'cm-1'
    }),
    0
  );

  const bound = bindStreamStarted(state, {
    event: 'stream_started',
    request_id: 'req-1',
    turn_id: 'server-t9',
    user_round: 1,
    resume_from_seq: 5,
    client_message_id: 'cm-1'
  });
  assert.equal(bound.turnId, 'server-t9');
  assert.equal(state.turns.has('local-t1'), false);
  assert.equal(state.turns.get('server-t9')?.clientMessageId, 'cm-1');
  assert.equal(state.turns.get('server-t9')?.userContent, 'typed text');
  assert.equal(state.turns.get('server-t9')?.userRound, 1);
  // The ack never advances the cursor; resume frames replay idempotently.
  assert.equal(state.lastSeq, 1);

  const created = bindStreamStarted(state, {
    event: 'stream_started',
    turn_id: 'server-t10',
    resume_from_seq: 6
  });
  assert.equal(created.turnId, 'server-t10');
  assert.equal(state.turns.get('server-t10')?.userContent, null);
  assert.equal(state.turns.get('server-t10')?.clientMessageId, null);
});

test('stream_started never writes bubble content from the ack (M2-A)', () => {
  const state = emptyChatThreadState('session-1');
  // A legacy server may still smuggle a content field into the ack; it must
  // be ignored — user content only arrives through the durable user item.
  const stray = { event: 'stream_started', turn_id: 'server-t11', resume_from_seq: 7, content: 'stray' };
  bindStreamStarted(state, stray as unknown as StreamStartedAck);
  assert.equal(state.turns.get('server-t11')?.userContent, null);
});

test('stream_started folds an already-canonical turn instead of clobbering it', () => {
  const state = emptyChatThreadState('session-1');
  applyChatThreadFrame(
    state,
    change(1, 'turn_upsert', { turn_id: 'local-t1', content: 'typed text', client_message_id: 'cm-1' }),
    0
  );
  // Resume replay delivered the canonical turn before the ack arrived.
  applyChatThreadFrame(
    state,
    change(2, 'turn_upsert', { turn_id: 'server-t9', user_round: 4, status: 'running', content: 'server text' }),
    0
  );

  const bound = bindStreamStarted(state, {
    event: 'stream_started',
    turn_id: 'server-t9',
    resume_from_seq: 2,
    client_message_id: 'cm-1'
  });
  assert.equal(bound.turnId, 'server-t9');
  assert.equal(state.turns.size, 1);
  assert.equal(state.turns.get('server-t9')?.userContent, 'server text');
  assert.equal(state.turns.get('server-t9')?.userRound, 4);
  assert.equal(state.turns.get('server-t9')?.status, 'running');
  assert.equal(state.turns.get('server-t9')?.clientMessageId, 'cm-1');
});

test('snapshot_required and overflow frames leave the state untouched', () => {
  const state = emptyChatThreadState('session-1');
  applyChatThreadFrame(state, upsertItem(1, { item_id: 'i1', content: 'live' }), 0);

  const snapshotRequired: ChatThreadFrame = { event: 'thread_snapshot_required', seq: 9 };
  const requiredResult = applyChatThreadFrame(state, snapshotRequired, 0);
  assert.deepEqual(
    { changed: requiredResult.changed, structural: requiredResult.structural, needResume: requiredResult.needResume, droppedEphemeral: requiredResult.droppedEphemeral },
    { changed: false, structural: false, needResume: false, droppedEphemeral: 0 }
  );

  const overflow: ChatThreadFrame = { event: 'stream_overflow', session_id: 'session-1', cursor: 9 };
  const overflowResult = applyChatThreadFrame(state, overflow, 0);
  assert.equal(overflowResult.changed, false);

  assert.equal(state.lastSeq, 1);
  assert.equal(state.items.get('i1')?.content, 'live');
  assert.equal(state.gap.length, 0);
});
