import test from 'node:test';
import assert from 'node:assert/strict';

import {
  createChatRuntimeProjection,
  applyChatRuntimeEvent
} from '../../src/realtime/chat/chatRuntimeReducer';
import {
  resolveMergedSessionBusy,
  resolveMergedSessionRuntimeStatus
} from '../../src/stores/chatBusyState';

test('merged busy state keeps running when projection is stale idle but runtime is running', () => {
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'sess_running',
    messages: [
      { role: 'user', content: 'input' },
      { role: 'assistant', content: 'partial answer' }
    ],
    loading: false,
    running: false
  });

  assert.equal(
    resolveMergedSessionBusy({
      projection,
      sessionId: 'sess_running',
      loading: false,
      messages: [
        { role: 'user', content: 'input' },
        { role: 'assistant', content: 'partial answer' }
      ],
      runtimeStatus: 'running',
      runtimeKnown: true,
      runtimeHasControllers: false
    }),
    true
  );
  assert.equal(
    resolveMergedSessionRuntimeStatus({
      projection,
      sessionId: 'sess_running',
      loading: false,
      messages: [
        { role: 'user', content: 'input' },
        { role: 'assistant', content: 'partial answer' }
      ],
      runtimeStatus: 'running',
      runtimeKnown: true,
      runtimeHasControllers: false
    }),
    'running'
  );
});

test('merged busy state suppresses stale assistant streaming after confirmed idle runtime', () => {
  const projection = createChatRuntimeProjection();
  const messages = [
    { role: 'user', content: 'input' },
    {
      role: 'assistant',
      content: 'done',
      stream_incomplete: true,
      workflowStreaming: true
    }
  ];
  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'sess_idle',
    messages,
    loading: true,
    running: true
  });

  assert.equal(
    resolveMergedSessionBusy({
      projection,
      sessionId: 'sess_idle',
      loading: false,
      messages,
      runtimeStatus: 'idle',
      runtimeKnown: true,
      runtimeHasControllers: false
    }),
    false
  );
  assert.equal(
    resolveMergedSessionRuntimeStatus({
      projection,
      sessionId: 'sess_idle',
      loading: false,
      messages,
      runtimeStatus: 'idle',
      runtimeKnown: true,
      runtimeHasControllers: false
    }),
    'idle'
  );
});

test('merged busy state suppresses stale busy projection after confirmed idle runtime', () => {
  const projection = createChatRuntimeProjection();
  const messages = [
    { role: 'user', content: 'input' },
    { role: 'assistant', content: 'done' }
  ];
  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'sess_stale_projection',
    messages,
    loading: true,
    running: true
  });

  assert.equal(
    resolveMergedSessionBusy({
      projection,
      sessionId: 'sess_stale_projection',
      loading: false,
      messages,
      runtimeStatus: 'idle',
      runtimeKnown: true,
      runtimeHasControllers: false
    }),
    false
  );
  assert.equal(
    resolveMergedSessionRuntimeStatus({
      projection,
      sessionId: 'sess_stale_projection',
      loading: false,
      messages,
      runtimeStatus: 'idle',
      runtimeKnown: true,
      runtimeHasControllers: false
    }),
    'idle'
  );
});
