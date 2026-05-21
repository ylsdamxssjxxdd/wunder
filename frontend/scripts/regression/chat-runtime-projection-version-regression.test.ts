import test from 'node:test';
import assert from 'node:assert/strict';

import { createChatRuntimeProjection } from '../../src/realtime/chat/chatRuntimeReducer';
import { buildCanonicalChatRuntimeEvents } from '../../src/realtime/chat/chatCanonicalEvents';
import { buildCanonicalClientMessageSubmittedEvent } from '../../src/realtime/chat/chatRuntimeBridge';
import {
  applyChatRuntimeEventsWithInvalidation,
  clearRuntimeProjectionInvalidation,
  markRuntimeProjectionChanged,
  runtimeProjectionInvalidationState
} from '../../src/realtime/chat/chatRuntimeProjectionInvalidation';

const createStore = () => ({
  activeSessionId: 'session-1',
  sessions: [{ id: 'session-1', agent_id: 'agent-1' }],
  messages: [],
  loadingBySession: {},
  runtimeProjection: createChatRuntimeProjection(),
  runtimeProjectionVersion: 0
});

const flushTimers = () => new Promise((resolve) => setTimeout(resolve, 25));

test('runtime projection invalidation batches non-immediate stream updates', async () => {
  const store = createStore();

  applyChatRuntimeEventsWithInvalidation(
    store,
    store.runtimeProjection,
    buildCanonicalChatRuntimeEvents({
      sessionId: 'session-1',
      eventType: 'llm_output_delta',
      payload: {
        event_id: 1,
        event_seq: 1,
        user_round: 1,
        delta: 'a'
      },
      eventId: 1,
      requestId: 'request-1',
      phase: 'send'
    }),
    { reason: 'stream:send' }
  );
  applyChatRuntimeEventsWithInvalidation(
    store,
    store.runtimeProjection,
    buildCanonicalChatRuntimeEvents({
      sessionId: 'session-1',
      eventType: 'llm_output_delta',
      payload: {
        event_id: 2,
        event_seq: 2,
        user_round: 1,
        delta: 'b'
      },
      eventId: 2,
      requestId: 'request-1',
      phase: 'send'
    }),
    { reason: 'stream:send' }
  );

  assert.equal(store.runtimeProjectionVersion, 0);
  assert.equal(runtimeProjectionInvalidationState.pending, true);

  await flushTimers();

  assert.equal(store.runtimeProjectionVersion, 1);
  assert.equal(runtimeProjectionInvalidationState.pending, false);
});

test('runtime projection invalidation ignores duplicate stream event ids', async () => {
  const store = createStore();
  const payload = {
    event_id: 1,
    event_seq: 1,
    user_round: 1,
    delta: 'a'
  };

  applyChatRuntimeEventsWithInvalidation(
    store,
    store.runtimeProjection,
    buildCanonicalChatRuntimeEvents({
      sessionId: 'session-1',
      eventType: 'llm_output_delta',
      payload,
      eventId: 1,
      requestId: 'request-1',
      phase: 'send'
    })
  );
  await flushTimers();
  assert.equal(store.runtimeProjectionVersion, 1);

  applyChatRuntimeEventsWithInvalidation(
    store,
    store.runtimeProjection,
    buildCanonicalChatRuntimeEvents({
      sessionId: 'session-1',
      eventType: 'llm_output_delta',
      payload,
      eventId: 1,
      requestId: 'request-1',
      phase: 'send'
    })
  );
  await flushTimers();

  assert.equal(store.runtimeProjectionVersion, 1);
});

test('runtime projection invalidation applies immediate boundary updates without waiting for a frame', () => {
  const store = createStore();

  applyChatRuntimeEventsWithInvalidation(
    store,
    store.runtimeProjection,
    [
      buildCanonicalClientMessageSubmittedEvent({
        sessionId: 'session-1',
        content: 'hello',
        clientMessageId: 'client-1'
      })
    ],
    { immediate: true }
  );

  assert.equal(store.runtimeProjectionVersion, 1);
  assert.equal(runtimeProjectionInvalidationState.pending, false);
});

test('runtime projection invalidation keeps pending gap visible without repainting stale content', async () => {
  const store = createStore();

  applyChatRuntimeEventsWithInvalidation(
    store,
    store.runtimeProjection,
    buildCanonicalChatRuntimeEvents({
      sessionId: 'session-1',
      eventType: 'llm_output_delta',
      payload: {
        event_id: 1,
        event_seq: 1,
        user_round: 1,
        delta: 'a'
      },
      eventId: 1,
      requestId: 'request-1',
      phase: 'watch'
    })
  );
  await flushTimers();
  assert.equal(store.runtimeProjectionVersion, 1);

  const results = applyChatRuntimeEventsWithInvalidation(
    store,
    store.runtimeProjection,
    buildCanonicalChatRuntimeEvents({
      sessionId: 'session-1',
      eventType: 'llm_output_delta',
      payload: {
        event_id: 3,
        event_seq: 3,
        user_round: 1,
        delta: 'c'
      },
      eventId: 3,
      requestId: 'request-1',
      phase: 'watch'
    })
  );
  await flushTimers();

  assert.equal(results[0]?.pending, true);
  assert.equal(results[0]?.reason, 'pending_event_seq_gap');
  assert.equal(store.runtimeProjectionVersion, 1);
  assert.equal(store.runtimeProjection.sessions['session-1'].syncRequired, true);
});

test('immediate runtime projection invalidation cancels a pending frame before bumping', () => {
  const store = createStore();

  markRuntimeProjectionChanged(store);
  assert.equal(runtimeProjectionInvalidationState.pending, true);

  markRuntimeProjectionChanged(store, { immediate: true });

  assert.equal(store.runtimeProjectionVersion, 1);
  assert.equal(runtimeProjectionInvalidationState.pending, false);
});

test.afterEach(() => {
  clearRuntimeProjectionInvalidation();
});
