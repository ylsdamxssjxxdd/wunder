import test from 'node:test';
import assert from 'node:assert/strict';
import { buildCanonicalChatRuntimeEvents } from '../../src/realtime/chat/chatCanonicalEvents';

for (const [source, expected] of [
  ['llm_output_delta', 'assistant_delta'],
  ['tool_output_delta', 'tool_call_delta'],
  ['tool_call_delta', 'tool_call_delta'],
  ['command_session_delta', 'tool_call_delta']
]) {
  test(`typed delta preserves ${source} routing`, () => {
    const options = {
      sessionId: 'thread', eventId: '5',
      payload: { data: { source_event: source, delta: 'text', tool_call_id: 'call', user_round: 1, model_round: 1 } }
    };
    const actual = buildCanonicalChatRuntimeEvents({ ...options, eventType: 'thread_item_delta' });
    assert.deepEqual(actual, buildCanonicalChatRuntimeEvents({ ...options, eventType: source }));
    assert.equal(actual[0].event_type, expected);
  });
}

test('unknown delta types cannot inject tool output into assistant prose', () => {
  for (const source of ['', 'thread_item_delta', 'unrecognized']) {
    assert.deepEqual(buildCanonicalChatRuntimeEvents({
      sessionId: 'thread', eventType: 'thread_item_delta',
      payload: { data: { source_event: source, delta: 'text' } }
    }), []);
  }
});
