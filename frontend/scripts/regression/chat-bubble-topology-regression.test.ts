import test from 'node:test';
import assert from 'node:assert/strict';

import { enforceOneBubblePerChatTurn } from '../../src/realtime/chat/chatBubbleTopology';

const row = (key: string, message: Record<string, unknown>) => ({ key, sourceIndex: 0, message });

test('chat surface enforces one user and one assistant bubble for a durable turn', () => {
  const rows = enforceOneBubblePerChatTurn([
    row('legacy-user', { role: 'user', user_turn_id: 'turn-1', content: 'request' }),
    row('legacy-assistant', {
      role: 'assistant', user_turn_id: 'turn-1', content: 'first fragment',
      status: 'streaming', workflowItems: [{ id: 'tool-1', status: 'loading' }]
    }),
    row('durable-user', {
      role: 'user', __runtime_projected: true, __runtime_user_turn_id: 'turn-1',
      content: 'request', attachments: [{ id: 'attachment-1' }]
    }),
    row('durable-assistant', {
      role: 'assistant', __runtime_projected: true, __runtime_user_turn_id: 'turn-1',
      content: 'final response', reasoning: 'final reasoning', status: 'final', final: true,
      stats: { interaction_duration_s: 1.5, visible_decode_speed_tps: 20 },
      workflowItems: [{ id: 'tool-1', status: 'completed' }, { id: 'tool-2', status: 'completed' }]
    }),
    row('stale-running', {
      role: 'assistant', user_turn_id: 'turn-1', content: '', status: 'streaming',
      stream_incomplete: true
    }),
    row('next-user', { role: 'user', user_turn_id: 'turn-2', content: 'next request' }),
    row('next-assistant', { role: 'assistant', user_turn_id: 'turn-2', content: 'next response', status: 'final' })
  ]);

  assert.deepEqual(rows.map((item) => item.key), [
    'durable-user', 'durable-assistant', 'next-user', 'next-assistant'
  ]);
  assert.equal(rows[1].message.status, 'final');
  assert.equal(rows[1].message.stream_incomplete, false);
  assert.equal(rows[1].message.content, 'first fragment\n\nfinal response');
  assert.equal(rows[1].message.reasoning, 'final reasoning');
  assert.deepEqual((rows[1].message.workflowItems as Array<Record<string, unknown>>).map((item) => item.id), ['tool-1', 'tool-2']);
  assert.deepEqual(rows[1].message.stats, {
    interaction_duration_s: 1.5,
    visible_decode_speed_tps: 20
  });
});

test('chat surface preserves one assistant-only greeting and folds optimistic rows into their user turn', () => {
  const rows = enforceOneBubblePerChatTurn([
    row('greeting-a', { role: 'assistant', isGreeting: true, content: 'Hello' }),
    row('greeting-b', { role: 'assistant', isGreeting: true, content: 'Hello' }),
    row('local-user', { role: 'user', content: 'draft request' }),
    row('local-assistant-a', { role: 'assistant', content: 'draft fragment', status: 'streaming' }),
    row('local-assistant-b', { role: 'assistant', content: 'draft completion', status: 'final' })
  ]);

  assert.deepEqual(rows.map((item) => item.key), ['greeting-a', 'local-user', 'local-assistant-b']);
  assert.equal(rows[2].message.content, 'draft fragment\n\ndraft completion');
  assert.equal(rows[2].message.status, 'final');
});


test('chat surface keeps the later model round live inside the same assistant bubble', () => {
  const rows = enforceOneBubblePerChatTurn([
    row('user', { role: 'user', user_turn_id: 'turn-loop', content: 'request' }),
    row('model-one', {
      role: 'assistant', user_turn_id: 'turn-loop', model_turn_id: 'model-one',
      content: 'first model result', status: 'final', final: true
    }),
    row('model-two', {
      role: 'assistant', user_turn_id: 'turn-loop', model_turn_id: 'model-two',
      content: 'second model is working', status: 'streaming', stream_incomplete: true
    })
  ]);

  assert.deepEqual(rows.map((item) => item.key), ['user', 'model-two']);
  assert.equal(rows[1].message.content, 'first model result\n\nsecond model is working');
  assert.equal(rows[1].message.status, 'streaming');
  assert.equal(rows[1].message.stream_incomplete, true);
});
