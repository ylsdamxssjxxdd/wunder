// AI生成
// The thinking body re-renders on the reasoning clock, and the fold reads that
// clock through the turn-level assistant row id. Pin the contract: a reasoning
// tail must bump `tturn:<turn>:assistant` within a couple of frames, otherwise
// the thought only refreshes when some unrelated content delta lands (the "stalls mid-sentence, then dumps everything" report) and different threads
// behave differently.
import test from 'node:test';
import assert from 'node:assert/strict';

import { applyChatThreadServerEvent, ensureChatThreadRuntime, resetChatThreadRuntime } from '../../src/realtime/chat/chatThreadRuntime';

const SESSION = 'fixture-thread';

const makeStore = () => ({
  activeSessionId: SESSION,
  runtimeProjectionVersionBySession: {} as Record<string, number>,
  runtimeProjectionContentVersionByMessage: {} as Record<string, number>,
  runtimeProjectionReasoningVersionByMessage: {} as Record<string, number>
});

test('a reasoning tail bumps the turn-level reasoning clock the fold subscribes to', async () => {
  resetChatThreadRuntime(SESSION);
  const store = makeStore();
  ensureChatThreadRuntime(SESSION);
  const apply = (type: string, payload: unknown) =>
    applyChatThreadServerEvent(store as never, SESSION, type, payload as never);

  apply('thread_change', {
    data: {
      change_type: 'turn_upsert', turn_id: 'turn-r', cursor: 1, revision: 1,
      payload: { turn_id: 'turn-r', root_turn_id: 'turn-r', status: 'running', user_round: 1, content: 'fixture input' }
    }
  });
  apply('thread_change', {
    data: {
      change_type: 'item_upsert', turn_id: 'turn-r', cursor: 2, revision: 1,
      item: {
        item_id: 'turn-r:text-1', turn_id: 'turn-r', kind: 'assistant_message',
        status: 'running', revision: 1, visibility: 'user',
        payload: { model_round: 1, role: 'assistant', content: '', reasoning: '' }
      }
    }
  });
  apply('thread_item_tail', {
    data: { item_id: 'turn-r:text-1', field: 'reasoning', offset: 0, text: 'fixture thought' }
  });
  // The thought must land within a couple of frames: a wider window made it
  // freeze mid-sentence and then dump the rest at once.
  const started = Date.now();
  while (Number(store.runtimeProjectionReasoningVersionByMessage['tturn:turn-r:assistant'] || 0) !== 1) {
    if (Date.now() - started > 120) break;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  const latencyMs = Date.now() - started;
  assert.ok(latencyMs <= 120, `reasoning clock must land within 120ms, took ${latencyMs}ms`);

  const clock = store.runtimeProjectionReasoningVersionByMessage;
  assert.equal(clock['tturn:turn-r:assistant'], 1, 'the reasoning clock is keyed by the assistant row id');
  assert.equal(store.runtimeProjectionContentVersionByMessage['tturn:turn-r:assistant'], undefined,
    'a reasoning tail must not bump the content clock');
  // A later content delta is a separate clock: the thought must not depend on it.
  apply('thread_item_tail', {
    data: { item_id: 'turn-r:text-1', field: 'content', offset: 0, text: 'fixture answer' }
  });
  await new Promise((resolve) => setTimeout(resolve, 80));
  assert.equal(store.runtimeProjectionContentVersionByMessage['tturn:turn-r:assistant'], 1);
  assert.equal(clock['tturn:turn-r:assistant'], 1);
  resetChatThreadRuntime(SESSION);
  console.log('reasoning clock: turn-level key bump within a frame budget, isolated from content pass');
});
