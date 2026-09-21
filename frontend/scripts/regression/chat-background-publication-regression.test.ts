import test from 'node:test';
import assert from 'node:assert/strict';
import { createChatRuntimeProjection } from '../../src/realtime/chat/chatRuntimeReducer';
import { applyChatRuntimeEventsWithInvalidation, clearRuntimeProjectionInvalidation } from '../../src/realtime/chat/chatRuntimeProjectionInvalidation';
import { flushBackgroundPublication } from '../../src/realtime/chat/chatBackgroundPublication';

test('background content is reduced immediately but UI publication is coalesced and flushed on return', async () => {
  clearRuntimeProjectionInvalidation();
  const store = { activeSessionId: 'session-1', foregroundChatSessionId: '',
    runtimeProjectionVersion: 0, runtimeProjectionVersionBySession: {} as Record<string, number>,
    runtimeProjectionContentVersion: 0, runtimeProjectionContentVersionByMessage: {} as Record<string, number> };
  const projection = createChatRuntimeProjection();
  const emit = (event_type: string, content: Record<string, unknown>) => applyChatRuntimeEventsWithInvalidation(
    store, projection, [{ event_type, session_id: 'session-1', source: 'test', strict: false,
      user_turn_id: 'turn-1', model_turn_id: 'model-1', message_id: 'message-1', ...content }]);
  emit('assistant_message_created', {});
  for (let index = 0; index < 100; index++) emit('assistant_delta', { delta: 'a' });
  assert.equal(projection.sessions['session-1'].messageById['message-1'].content, 'a'.repeat(100));
  assert.equal(store.runtimeProjectionVersion, 0);
  assert.equal(store.runtimeProjectionContentVersion, 0);
  flushBackgroundPublication(store);
  assert.equal(store.runtimeProjectionVersion, 1);
  assert.equal(store.runtimeProjectionVersionBySession['session-1'], 1);
  assert.equal(store.runtimeProjectionContentVersionByMessage['message-1'], 1);
  emit('assistant_delta', { delta: 'b' });
  emit('assistant_final', { content: 'done' });
  await new Promise(resolve => setTimeout(resolve, 60));
  assert.equal(projection.sessions['session-1'].messageById['message-1'].status, 'final');
  assert.ok(store.runtimeProjectionVersion > 1);
  clearRuntimeProjectionInvalidation();
});


test('background structural updates do not invalidate the foreground session and use a bounded publication delay', async (context) => {
  clearRuntimeProjectionInvalidation();
  context.after(clearRuntimeProjectionInvalidation);
  const store = { activeSessionId: 'session-1', foregroundChatSessionId: 'session-1',
    runtimeProjectionVersion: 0, runtimeProjectionVersionBySession: {} as Record<string, number>,
    runtimeProjectionContentVersion: 0, runtimeProjectionContentVersionByMessage: {} as Record<string, number> };
  const projection = createChatRuntimeProjection();
  const emit = (sessionId: string, event_type: string, index: number) => applyChatRuntimeEventsWithInvalidation(
    store, projection, [{ event_type, session_id: sessionId, source: 'test', strict: false,
      user_turn_id: `turn-${sessionId}-${index}`, model_turn_id: `model-${sessionId}-${index}`, message_id: `${sessionId}-${index}`,
      delta: 'x' }]);
  for (let index = 0; index < 100; index++) emit('session-2', 'assistant_message_created', index);
  emit('session-1', 'assistant_message_created', 0);
  await new Promise(resolve => setTimeout(resolve, 70));
  assert.equal(store.runtimeProjectionVersionBySession['session-2'], undefined);
  const foregroundVersion = store.runtimeProjectionVersionBySession['session-1'];
  assert.equal(foregroundVersion, 1);
  await new Promise(resolve => setTimeout(resolve, 240));
  assert.equal(store.runtimeProjectionVersionBySession['session-2'], 1);
  assert.equal(store.runtimeProjectionVersionBySession['session-1'], foregroundVersion);
  assert.equal(projection.sessions['session-2'].messages.length, 100);
});
