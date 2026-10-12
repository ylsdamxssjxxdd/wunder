// W3-I projection memoization regression.
//
// Each test carries a negative assertion that fails against the pre-W3-I
// implementation: before memoization `buildChatThreadTurnSlots` minted fresh
// slot/bubble objects on every call, and the v2 bubbles had no
// `structureVersion`, so chatRuntimeRenderAdapter's O(1) identity skip was
// unreachable on the v2 path.
import test from 'node:test';
import assert from 'node:assert/strict';

import { emptyChatThreadState } from '../../src/realtime/chat/chatThreadTypes';
import type { ChatThreadFrame } from '../../src/realtime/chat/chatThreadTypes';
import { applyChatThreadFrame } from '../../src/realtime/chat/chatThreadState';
import { buildChatThreadTurnSlots } from '../../src/realtime/chat/chatThreadProjection';
import {
  getChatRuntimeIdentitySkipCount,
  materializeChatRuntimeProjectionList,
  resetChatRuntimeIdentitySkipCount
} from '../../src/realtime/chat/chatRuntimeRenderAdapter';
import type { ChatRuntimeProjection } from '../../src/realtime/chat/chatRuntimeTypes';

const change = (
  seq: number,
  changeType: string,
  data: Record<string, unknown>
): ChatThreadFrame => ({ event: 'thread_change', seq, change_type: changeType, data });

// Two accepted user turns: turn-a settled, turn-b still streaming.
const buildFixtureState = () => {
  const state = emptyChatThreadState('memo-fixture-session');
  const frames: ChatThreadFrame[] = [
    change(1, 'turn_upsert', { turn_id: 'turn-a', user_round: 1, status: 'completed', content: 'Fixture input a' }),
    change(2, 'item_upsert', { item_id: 'turn-a:user', turn_id: 'turn-a', kind: 'user_message', role: 'user',
      visibility: 'user', status: 'completed', content: 'Fixture input a', user_round: 1, revision: 1 }),
    change(3, 'item_upsert', { item_id: 'turn-a:text-1', turn_id: 'turn-a', model_round: 1, kind: 'assistant_message',
      role: 'assistant', visibility: 'user', status: 'completed', content: 'Fixture answer a', revision: 1 }),
    change(4, 'turn_upsert', { turn_id: 'turn-b', user_round: 2, status: 'running', content: 'Fixture input b' }),
    change(5, 'item_upsert', { item_id: 'turn-b:user', turn_id: 'turn-b', kind: 'user_message', role: 'user',
      visibility: 'user', status: 'completed', content: 'Fixture input b', user_round: 2, revision: 1 }),
    change(6, 'item_upsert', { item_id: 'turn-b:text-1', turn_id: 'turn-b', model_round: 1, kind: 'assistant_message',
      role: 'assistant', visibility: 'user', status: 'running', content: '', revision: 1 })
  ];
  frames.forEach((frame) => applyChatThreadFrame(state, frame, 0));
  return state;
};

test('bumping one turn preserves every untouched slot object identity', () => {
  const state = buildFixtureState();
  const before = buildChatThreadTurnSlots(state);
  assert.deepEqual(before.map((slot) => slot.rootTurnId), ['turn-a', 'turn-b']);

  // Touch only turn-b, and only through an ephemeral tail: no revision bump
  // anywhere, so the change is visible only through composed text.
  applyChatThreadFrame(state, {
    event: 'thread_item_tail', item_id: 'turn-b:text-1', field: 'content', offset: 0, text: 'streaming b'
  }, 0);

  const after = buildChatThreadTurnSlots(state);
  assert.equal(after.length, 2);
  // Negative assertion: the pre-W3-I full rebuild minted a new slot, user
  // bubble and assistant bubble for every turn on every call, so these
  // identity checks fail without per-turn memoization.
  assert.equal(after[0], before[0], 'untouched turn-a slot must be reused verbatim');
  assert.equal(after[0].user, before[0].user, 'untouched user bubble identity must be stable');
  assert.equal(after[0].assistant, before[0].assistant, 'untouched assistant bubble identity must be stable');
  assert.equal(after[0].user.structureVersion, before[0].user.structureVersion);
  assert.notEqual(after[1], before[1], 'the bumped turn-b slot must be rebuilt');
  assert.equal(after[1].assistant.content, 'streaming b');
});

test('a revision bump rebuilds only the changed turn slot', () => {
  const state = buildFixtureState();
  const before = buildChatThreadTurnSlots(state);

  applyChatThreadFrame(state, change(7, 'item_upsert', {
    item_id: 'turn-a:text-1', turn_id: 'turn-a', model_round: 1, kind: 'assistant_message',
    role: 'assistant', visibility: 'user', status: 'completed', content: 'Fixture answer a revised', revision: 2
  }), 0);

  const after = buildChatThreadTurnSlots(state);
  assert.notEqual(after[0], before[0], 'changed turn-a must be rebuilt');
  assert.equal(after[0].assistant.content, 'Fixture answer a revised');
  assert.equal(after[1], before[1], 'untouched turn-b must be reused');
});

test('v2 projection bubbles reach the adapter O(1) identity skip', () => {
  const state = buildFixtureState();
  const slots = buildChatThreadTurnSlots(state);
  const messages = slots.flatMap((slot) => [slot.user, slot.assistant]);

  const projection = {} as ChatRuntimeProjection;
  const sessionId = 'memo-render-session';
  const first = materializeChatRuntimeProjectionList(projection, sessionId, messages, {
    trustProjectionVersions: true
  });
  resetChatRuntimeIdentitySkipCount();
  const second = materializeChatRuntimeProjectionList(projection, sessionId, messages, {
    trustProjectionVersions: true
  });

  // Negative assertion: with an undefined structureVersion the adapter's skip
  // branch is unreachable, so this count stays 0 even though the message
  // objects are identical across the two calls.
  assert.ok(getChatRuntimeIdentitySkipCount() > 0, 'the identity-skip branch must be reachable on the v2 path');
  assert.equal(second[0], first[0], 'settled user row is reused');
  assert.equal(second[1], first[1], 'settled assistant row is reused');
  // And every v2 bubble carries the version the skip condition requires.
  for (const message of messages) {
    assert.equal(typeof message.structureVersion, 'number', `${message.id} must carry a structureVersion`);
  }
});

test('projection output stays deep-equal across calls (still a pure function of state)', () => {
  const state = buildFixtureState();
  const first = buildChatThreadTurnSlots(state);
  const second = buildChatThreadTurnSlots(state);
  assert.deepEqual(second, first);
  // Only the array wrapper churns; untouched slot identities are shared.
  assert.equal(second[0], first[0]);
  assert.equal(second[1], first[1]);
});
