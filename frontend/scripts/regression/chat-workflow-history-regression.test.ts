import test from 'node:test';
import assert from 'node:assert/strict';
import { createChatRuntimeProjection, applyChatRuntimeEvent } from '../../src/realtime/chat/chatRuntimeReducer';
import { buildChatRuntimeRenderableMessages } from '../../src/realtime/chat/chatRuntimeRenderAdapter';

test('terminal workflow rows survive cache eviction without re-reading historical payloads', () => {
  const projection = createChatRuntimeProjection();
  const sessionId = 'session-1';
  applyChatRuntimeEvent(projection, {
    event_type: 'session_snapshot', source: 'snapshot', strict: false, session_id: sessionId,
    running: false,
    messages: Array.from({ length: 400 }, (_, index) => ({
      role: 'assistant', message_id: `message-${index}`, user_turn_id: `turn-${index}`,
      model_turn_id: `model-${index}`, turn_index: index + 1, content: `text-${index}`,
      workflowItems: [{ id: `tool-${index}`, eventType: 'tool_result', status: 'completed', detail: 'result' }]
    }))
  });
  const first = buildChatRuntimeRenderableMessages({ projection, sessionId });
  let reads = 0;
  for (const message of Object.values(projection.sessions[sessionId].messageById)) {
    for (const item of message.workflowItems) {
      Object.defineProperty(item, 'detail', { configurable: true, enumerable: true,
        get: () => { reads++; return 'result'; } });
    }
  }
  const second = buildChatRuntimeRenderableMessages({ projection, sessionId });
  assert.equal(second.length, 400);
  assert.equal(reads, 0);
  assert.ok(second.every((row, index) => row.message === first[index].message));
  assert.ok(second.every(row => (row.message.workflowItems as unknown[]).length === 1));

  // Late history hydration must still invalidate a terminal row through its clock.
  const changed = projection.sessions[sessionId].messageById['message-0'];
  changed.workflowItems.push({ id: 'late-tool', eventType: 'tool_result', status: 'completed' });
  changed.structureVersion = Number(changed.structureVersion || 0) + 1;
  const refreshed = buildChatRuntimeRenderableMessages({ projection, sessionId });
  assert.equal((refreshed[0].message.workflowItems as unknown[]).length, 2);
  assert.equal(refreshed[1].message, first[1].message);
});
