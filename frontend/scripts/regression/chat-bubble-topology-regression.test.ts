import test from 'node:test';
import assert from 'node:assert/strict';
import { emptyChatThreadState } from '../../src/realtime/chat/chatThreadTypes';
import { applyChatThreadFrame, applyChatThreadSnapshot } from '../../src/realtime/chat/chatThreadState';
import { buildChatThreadTurnSlots } from '../../src/realtime/chat/chatThreadProjection';

// Adversarial transport order, not a second implementation of the renderer.
test('fixed turn slots survive reordered, duplicated execution records at every frame', () => {
  for (let seed = 1; seed <= 24; seed++) {
    const state = emptyChatThreadState('fixture-thread');
    const frames: any[] = [];
    const emit = (change_type: string, data: Record<string, unknown>) => frames.push({
      event: 'thread_change', seq: frames.length + 1, change_type, data
    });
    for (let round = 1; round <= 4; round++) {
      const root = `fixture-root-${round}`;
      emit('turn_upsert', { turn_id: root, root_turn_id: root, user_round: round, status: 'queued' });
      emit('item_upsert', { item_id: `${root}:user`, turn_id: root, kind: 'user_message',
        content: `Fixture input ${round}`, user_round: round, revision: 1, visibility: 'user' });
      emit('turn_status', { turn_id: root, status: 'completed' });
      for (let execution = 1; execution <= 3; execution++) {
        const child = `${root}-execution-${execution}`;
        emit('turn_upsert', { turn_id: child, root_turn_id: root, user_round: round, status: 'running' });
        emit('item_upsert', { item_id: `${child}:user`, turn_id: child, root_turn_id: root,
          kind: 'user_message', content: 'Internal continuation', visibility: 'model_internal', revision: 1 });
        emit('item_upsert', { item_id: `${child}:text-1`, turn_id: child, root_turn_id: root,
          kind: 'assistant_message', model_round: 1, content: 'Fixture answer', status: 'completed', revision: 1 });
        emit('turn_status', { turn_id: child, status: execution === 3 && round === 2 ? 'cancelled' : 'completed' });
      }
    }
    let random = seed;
    const next = () => (random = (random * 1664525 + 1013904223) >>> 0);
    const check = () => {
      const slots = buildChatThreadTurnSlots(state);
      assert.equal(new Set(slots.map(slot => slot.rootTurnId)).size, slots.length);
      for (const slot of slots) {
        assert.match(slot.rootTurnId, /^fixture-root-[1-4]$/);
        assert.equal(slot.user.role, 'user');
        assert.equal(slot.assistant.role, 'assistant');
        assert.equal(slot.user.userTurnId, slot.rootTurnId);
        assert.equal(slot.assistant.userTurnId, slot.rootTurnId);
      }
    };
    for (let offset = 0; offset < frames.length; offset += 8) {
      const chunk = frames.slice(offset, offset + 8);
      for (let i = chunk.length - 1; i > 0; i--) {
        const j = next() % (i + 1); [chunk[i], chunk[j]] = [chunk[j], chunk[i]];
      }
      for (const frame of chunk) {
        applyChatThreadFrame(state, frame, 0); check();
        applyChatThreadFrame(state, frame, 0); check();
      }
    }
    assert.equal(buildChatThreadTurnSlots(state).length, 4);
  }
});

test('unowned assistant/tool rows cannot create page structure; empty user input can', () => {
  const state = emptyChatThreadState('fixture-thread');
  applyChatThreadSnapshot(state, { cursor: 10, turns: [{ turn_id: 'orphan', status: 'completed' }],
    items: [{ item_id: 'orphan:text-1', turn_id: 'orphan', kind: 'assistant_message', model_round: 1,
      content: 'Fixture orphan', revision: 1 }], blocks: [] }, 0);
  assert.deepEqual(buildChatThreadTurnSlots(state), []);
  applyChatThreadFrame(state, { event: 'thread_change', seq: 11, change_type: 'item_upsert', data: {
    item_id: 'root:user', turn_id: 'root', kind: 'user_message', content: '', revision: 1,
    attachments: [{ name: 'fixture.txt' }]
  } }, 0);
  const [slot] = buildChatThreadTurnSlots(state);
  assert.equal(slot.rootTurnId, 'root');
  assert.equal(slot.user.content, '');
  assert.equal(slot.assistant.role, 'assistant');
});
