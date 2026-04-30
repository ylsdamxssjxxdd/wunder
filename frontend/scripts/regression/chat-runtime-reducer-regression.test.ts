import test from 'node:test';
import assert from 'node:assert/strict';

import { createChatRuntimeProjection, applyChatRuntimeEvent } from '../../src/realtime/chat/chatRuntimeReducer';
import {
  selectSessionBusy,
  selectSessionRuntimeStatus,
  selectLegacyMessageStatus,
  selectVisibleMessageProjections
} from '../../src/realtime/chat/chatRuntimeSelectors';
import type { ChatRuntimeEvent } from '../../src/realtime/chat/chatRuntimeTypes';

const baseEvent = (overrides: ChatRuntimeEvent): ChatRuntimeEvent => ({
  source: 'test',
  strict: true,
  session_id: 'session-1',
  agent_id: 'agent-1',
  created_at: '2026-04-30T02:14:06.000Z',
  ...overrides
});

test('chat runtime projection renders late user sideband before its assistant turn', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_message_created',
    event_id: 'evt-1',
    event_seq: 1,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_final',
    event_id: 'evt-2',
    event_seq: 2,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    content: '50000'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'user_message_created',
    event_id: 'evt-3',
    event_seq: 3,
    user_turn_id: 'ut-1',
    message_id: 'um-1',
    content: 'how many people'
  }));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['user:how many people', 'assistant:50000']
  );
});

test('chat runtime reducer ignores duplicate final events by event id', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'user_message_created',
    event_id: 'evt-1',
    event_seq: 1,
    user_turn_id: 'ut-1',
    message_id: 'um-1',
    content: 'hello'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_message_created',
    event_id: 'evt-2',
    event_seq: 2,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_final',
    event_id: 'evt-3',
    event_seq: 3,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    content: 'done'
  }));
  const duplicate = applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_final',
    event_id: 'evt-3',
    event_seq: 4,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    content: 'done again'
  }));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(duplicate.ignored, true);
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['user:hello', 'assistant:done']
  );
});

test('chat runtime reducer keeps live message content when an older snapshot arrives later', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'user_message_created',
    event_id: 'evt-10',
    event_seq: 10,
    user_turn_id: 'ut-1',
    message_id: 'um-1',
    content: 'question'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-20',
    event_seq: 20,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    delta: 'live answer'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'session_snapshot',
    event_id: 'evt-21',
    event_seq: 21,
    snapshot_seq: 15,
    messages: [
      {
        message_id: 'um-1',
        role: 'user',
        content: 'question',
        user_turn_id: 'ut-1',
        created_at: '2026-04-30T02:14:06.000Z'
      },
      {
        message_id: 'am-1',
        role: 'assistant',
        content: 'old answer',
        user_turn_id: 'ut-1',
        model_turn_id: 'mt-1',
        created_at: '2026-04-30T02:14:07.000Z'
      }
    ]
  }));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible[1].content, 'live answer');
  assert.equal(projection.sessions['session-1'].appliedSeq, 21);
});

test('terminal turn event clears session busy state', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'user_message_created',
    event_id: 'evt-1',
    event_seq: 1,
    user_turn_id: 'ut-1',
    message_id: 'um-1',
    content: 'hello'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_message_created',
    event_id: 'evt-2',
    event_seq: 2,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1'
  }));

  assert.equal(selectSessionBusy(projection, 'session-1'), true);

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'turn_completed',
    event_id: 'evt-3',
    event_seq: 3,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1'
  }));

  assert.equal(selectSessionBusy(projection, 'session-1'), false);
  assert.equal(selectSessionRuntimeStatus(projection, 'session-1'), 'idle');
});

test('chat runtime reducer keeps appliedSeq monotonic and ignores stale events', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'session_runtime',
    event_id: 'evt-5',
    event_seq: 5,
    runtime_status: 'running'
  }));
  const stale = applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'session_runtime',
    event_id: 'evt-4',
    event_seq: 4,
    runtime_status: 'idle'
  }));

  assert.equal(stale.ignored, true);
  assert.equal(projection.sessions['session-1'].appliedSeq, 5);
  assert.equal(selectSessionBusy(projection, 'session-1'), true);
});

test('strict runtime events with missing ids are quarantined', () => {
  const projection = createChatRuntimeProjection();

  const result = applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_final',
    event_id: 'evt-1',
    event_seq: 1,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    content: 'done'
  }));

  const session = projection.sessions['session-1'];
  assert.equal(result.quarantined, true);
  assert.equal(session.quarantinedEvents.length, 1);
  assert.equal(session.messages.length, 0);
});

test('legacy projection clears stale streaming flags after terminal reconcile', () => {
  const projection = createChatRuntimeProjection();
  const staleAssistant = {
    role: 'assistant',
    content: 'done',
    stream_event_id: 9,
    stream_incomplete: true,
    workflowStreaming: true
  };

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [
      {
        role: 'user',
        content: 'hello',
        stream_event_id: 8
      },
      staleAssistant
    ],
    loading: true,
    running: true
  });
  assert.equal(selectSessionBusy(projection, 'session-1'), true);

  staleAssistant.stream_incomplete = false;
  staleAssistant.workflowStreaming = false;
  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [
      {
        role: 'user',
        content: 'hello',
        stream_event_id: 8
      },
      staleAssistant
    ],
    loading: false,
    running: false
  });

  assert.equal(selectSessionBusy(projection, 'session-1'), false);
  assert.equal(selectLegacyMessageStatus(projection, 'session-1', staleAssistant), 'final');
});

test('visible projection order can map back to original raw message references', () => {
  const projection = createChatRuntimeProjection();
  const assistant = {
    message_id: 'am-1',
    role: 'assistant',
    content: 'answer',
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1'
  };
  const user = {
    message_id: 'um-1',
    role: 'user',
    content: 'question',
    user_turn_id: 'ut-1'
  };

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [assistant, user],
    loading: false,
    running: false
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => message.raw),
    [user, assistant]
  );
});

test('legacy reconcile uses timestamps to bind a reversed assistant after its user turn', () => {
  const projection = createChatRuntimeProjection();
  const assistant = {
    message_id: 'am-weak',
    role: 'assistant',
    content: '42',
    created_at: '2026-04-30T02:14:08.000Z'
  };
  const user = {
    message_id: 'um-weak',
    role: 'user',
    content: 'count',
    created_at: '2026-04-30T02:14:06.000Z'
  };

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [assistant, user],
    loading: false,
    running: false
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['user:count', 'assistant:42']
  );
  assert.equal(visible[1].userTurnId, visible[0].userTurnId);
});

test('legacy reconcile keeps same stream_round user before assistant even when array order is reversed', () => {
  const projection = createChatRuntimeProjection();
  const assistant = {
    role: 'assistant',
    content: 'round answer',
    stream_round: 7,
    created_at: '2026-04-30T02:14:06.000Z'
  };
  const user = {
    role: 'user',
    content: 'round question',
    stream_round: 7,
    created_at: '2026-04-30T02:14:07.000Z'
  };

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [assistant, user],
    loading: false,
    running: false
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['user:round question', 'assistant:round answer']
  );
  assert.equal(visible[1].userTurnId, visible[0].userTurnId);
});

test('legacy reconcile without timestamps keeps deterministic message ids across refreshes', () => {
  const projection = createChatRuntimeProjection();
  const messages = [
    {
      role: 'user',
      content: 'stable question'
    },
    {
      role: 'assistant',
      content: 'stable answer'
    }
  ];

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages,
    loading: false,
    running: false
  });
  const firstVisible = selectVisibleMessageProjections(projection, 'session-1');
  const firstIds = firstVisible.map((message) => message.id);

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages,
    loading: false,
    running: false
  });

  const secondVisible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    secondVisible.map((message) => `${message.role}:${message.content}`),
    ['user:stable question', 'assistant:stable answer']
  );
  assert.deepEqual(secondVisible.map((message) => message.id), firstIds);
  assert.equal(secondVisible.length, 2);
});
