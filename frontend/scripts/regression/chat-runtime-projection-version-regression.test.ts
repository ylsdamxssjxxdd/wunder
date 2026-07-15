import test from 'node:test';
import assert from 'node:assert/strict';

import { createChatRuntimeProjection } from '../../src/realtime/chat/chatRuntimeReducer';
import { buildCanonicalChatRuntimeEvents } from '../../src/realtime/chat/chatCanonicalEvents';
import { buildCanonicalClientMessageSubmittedEvent } from '../../src/realtime/chat/chatRuntimeBridge';
import {
  applyChatRuntimeEventsWithInvalidation,
  clearRuntimeProjectionInvalidation,
  markRuntimeProjectionChanged,
  runtimeProjectionContentInvalidationState,
  runtimeProjectionInvalidationState
} from '../../src/realtime/chat/chatRuntimeProjectionInvalidation';

const createStore = () => ({
  activeSessionId: 'session-1',
  sessions: [{ id: 'session-1', agent_id: 'agent-1' }],
  messages: [],
  loadingBySession: {},
  runtimeProjection: createChatRuntimeProjection(),
  runtimeProjectionVersion: 0,
  runtimeProjectionContentVersion: 0,
  runtimeProjectionContentVersionByMessage: {}
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

test('runtime projection invalidation isolates steady assistant text deltas to content clock', async () => {
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
        model_round: 1,
        delta: 'a'
      },
      eventId: 1,
      requestId: 'request-1',
      phase: 'send'
    }),
    { reason: 'stream:send' }
  );
  await flushTimers();
  assert.equal(store.runtimeProjectionVersion, 1);
  assert.equal(store.runtimeProjectionContentVersion, 0);

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
        model_round: 1,
        delta: 'b'
      },
      eventId: 2,
      requestId: 'request-1',
      phase: 'send'
    }),
    { reason: 'stream:send' }
  );
  await flushTimers();

  const messageId = 'assistant-message:model-turn:session-1:user:1:model:1';
  assert.equal(store.runtimeProjectionVersion, 1);
  assert.equal(store.runtimeProjectionContentVersion, 1);
  assert.equal(store.runtimeProjectionContentVersionByMessage[messageId], 1);
});

test('runtime projection invalidation coalesces bursty steady assistant text deltas', async () => {
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
        model_round: 1,
        delta: 'a'
      },
      eventId: 1,
      requestId: 'request-1',
      phase: 'send'
    }),
    { reason: 'stream:send' }
  );
  await flushTimers();

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
        model_round: 1,
        delta: 'b'
      },
      eventId: 2,
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
        event_id: 3,
        event_seq: 3,
        user_round: 1,
        model_round: 1,
        delta: 'c'
      },
      eventId: 3,
      requestId: 'request-1',
      phase: 'send'
    }),
    { reason: 'stream:send' }
  );

  const messageId = 'assistant-message:model-turn:session-1:user:1:model:1';
  assert.equal(
    store.runtimeProjection.sessions['session-1'].messageById[messageId]?.content,
    'abc'
  );
  assert.equal(store.runtimeProjectionVersion, 1);
  assert.equal(store.runtimeProjectionContentVersion, 0);
  assert.equal(store.runtimeProjectionContentVersionByMessage[messageId], undefined);
  assert.equal(runtimeProjectionContentInvalidationState.pending, true);

  await flushTimers();

  assert.equal(store.runtimeProjectionVersion, 1);
  assert.equal(store.runtimeProjectionContentVersion, 1);
  assert.equal(store.runtimeProjectionContentVersionByMessage[messageId], 1);
  assert.equal(runtimeProjectionContentInvalidationState.pending, false);
});

test('runtime projection invalidation isolates steady tool output deltas to the message clock', async () => {
  const store = createStore();
  const initialEvent = (eventId: number, output: string) => buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'command_session_delta',
    eventId,
    requestId: 'request-tool-output',
    phase: 'send',
    payload: {
      event_id: eventId,
      event_seq: eventId,
      user_round: 1,
      model_round: 1,
      command_session_id: 'command-1',
      tool_call_id: 'tool-1',
      tool: 'execute_command',
      output
    }
  }).map((event) => ({
    ...event,
    payload: {
      ...event.payload,
      command_session_id: 'command-1',
      tool_call_id: 'tool-1'
    }
  }));

  applyChatRuntimeEventsWithInvalidation(
    store,
    store.runtimeProjection,
    initialEvent(1, 'first'),
    { reason: 'stream:send' }
  );
  await flushTimers();
  assert.equal(store.runtimeProjectionVersion, 1);

  applyChatRuntimeEventsWithInvalidation(
    store,
    store.runtimeProjection,
    initialEvent(2, 'second'),
    { reason: 'stream:send' }
  );
  await flushTimers();

  const [messageId, assistant] = Object.entries(store.runtimeProjection.sessions['session-1'].messageById)
    .find(([, message]) => message.role === 'assistant') || [];
  assert.ok(messageId);
  assert.ok(assistant);
  assert.equal(store.runtimeProjectionVersion, 1);
  assert.equal(store.runtimeProjectionContentVersion, 1);
  assert.equal(store.runtimeProjectionContentVersionByMessage[messageId], 1);
  assert.equal(
    assistant.workflowItems?.[0]?.detail.includes('second'),
    true
  );
});

test('runtime projection content invalidation has a timer fallback when animation frames stall', async () => {
  const store = createStore();
  const originalRequestAnimationFrame = globalThis.requestAnimationFrame;
  const originalCancelAnimationFrame = globalThis.cancelAnimationFrame;
  const hadRequestAnimationFrame = 'requestAnimationFrame' in globalThis;
  const hadCancelAnimationFrame = 'cancelAnimationFrame' in globalThis;
  (globalThis as typeof globalThis & {
    requestAnimationFrame?: (callback: FrameRequestCallback) => number;
    cancelAnimationFrame?: (handle: number) => void;
  }).requestAnimationFrame = () => 1;
  (globalThis as typeof globalThis & {
    cancelAnimationFrame?: (handle: number) => void;
  }).cancelAnimationFrame = () => undefined;

  try {
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
          model_round: 1,
          delta: 'a'
        },
        eventId: 1,
        requestId: 'request-1',
        phase: 'send'
      }),
      { reason: 'stream:send' }
    );
    await flushTimers();

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
          model_round: 1,
          delta: 'b'
        },
        eventId: 2,
        requestId: 'request-1',
        phase: 'send'
      }),
      { reason: 'stream:send' }
    );

    const messageId = 'assistant-message:model-turn:session-1:user:1:model:1';
    assert.equal(runtimeProjectionContentInvalidationState.pending, true);
    await flushTimers();
    assert.equal(store.runtimeProjectionContentVersion, 1);
    assert.equal(store.runtimeProjectionContentVersionByMessage[messageId], 1);
    assert.equal(runtimeProjectionContentInvalidationState.pending, false);
  } finally {
    if (hadRequestAnimationFrame) {
      (globalThis as typeof globalThis & {
        requestAnimationFrame?: typeof requestAnimationFrame;
      }).requestAnimationFrame = originalRequestAnimationFrame;
    } else {
      delete (globalThis as typeof globalThis & { requestAnimationFrame?: unknown }).requestAnimationFrame;
    }
    if (hadCancelAnimationFrame) {
      (globalThis as typeof globalThis & {
        cancelAnimationFrame?: typeof cancelAnimationFrame;
      }).cancelAnimationFrame = originalCancelAnimationFrame;
    } else {
      delete (globalThis as typeof globalThis & { cancelAnimationFrame?: unknown }).cancelAnimationFrame;
    }
  }
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
