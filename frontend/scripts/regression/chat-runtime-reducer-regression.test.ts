import test from 'node:test';
import assert from 'node:assert/strict';

import { createChatRuntimeProjection, applyChatRuntimeEvent } from '../../src/realtime/chat/chatRuntimeReducer';
import { buildCanonicalChatRuntimeEvents } from '../../src/realtime/chat/chatCanonicalEvents';
import {
  buildCanonicalClientMessageSubmittedEvent,
  buildCanonicalSessionEventsSnapshot
} from '../../src/realtime/chat/chatRuntimeBridge';
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

test('authoritative stopped legacy reconcile prunes projection-only assistant turns', () => {
  const projection = createChatRuntimeProjection();
  const userTurnId = 'user-turn:session-1:round:1';
  const firstModelTurnId = 'model-turn:session-1:user:1:model:1';
  const localUser = {
    role: 'user',
    content: 'stop this run',
    client_message_id: 'local-user-1',
    user_turn_id: userTurnId,
    stream_round: 1
  };
  const visibleAssistant = {
    role: 'assistant',
    content: 'visible run',
    user_turn_id: userTurnId,
    model_turn_id: firstModelTurnId,
    stream_round: 1,
    stream_incomplete: true,
    workflowStreaming: true
  };

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [localUser, visibleAssistant],
    loading: true,
    running: true
  });
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_final',
    event_id: 'evt-10',
    event_seq: 10,
    user_turn_id: userTurnId,
    model_turn_id: firstModelTurnId,
    message_id: 'assistant-first-model-turn',
    content: 'visible run'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-11',
    event_seq: 11,
    user_turn_id: userTurnId,
    model_turn_id: 'model-turn:session-1:user:1:model:2',
    message_id: 'assistant-second-model-turn',
    delta: 'projection only'
  }));

  const visibleBeforeStop = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visibleBeforeStop.length, 3);
  assert.equal(
    visibleBeforeStop.filter((message) => message.role === 'assistant').length,
    2
  );

  visibleAssistant.status = 'cancelled';
  visibleAssistant.cancelled = true;
  visibleAssistant.stop_reason = 'user_stop';
  visibleAssistant.stream_incomplete = false;
  visibleAssistant.workflowStreaming = false;
  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [localUser, visibleAssistant],
    loading: false,
    running: false,
    authoritative: true
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['user:stop this run', 'assistant:visible run']
  );
  assert.equal(visible[1].status, 'cancelled');
  assert.equal(selectSessionBusy(projection, 'session-1'), false);
});

test('late assistant events do not resurrect a cancelled user turn after local stop', () => {
  const projection = createChatRuntimeProjection();
  const userTurnId = 'user-turn:session-1:round:1';
  const modelTurnId = 'model-turn:session-1:user:1:model:1';

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'user_message_created',
    event_id: 'evt-user-stop',
    event_seq: 1,
    user_turn_id: userTurnId,
    message_id: 'user-message-stop',
    content: 'cancel this'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-before-stop',
    event_seq: 2,
    user_turn_id: userTurnId,
    model_turn_id: modelTurnId,
    message_id: 'assistant-message-stop',
    delta: 'partial'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'turn_cancelled',
    event_id: 'evt-stop',
    event_seq: 3,
    user_turn_id: userTurnId,
    model_turn_id: modelTurnId
  }));

  const late = applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-late-after-stop',
    event_seq: 4,
    user_turn_id: userTurnId,
    model_turn_id: 'model-turn:session-1:user:1:model:2',
    message_id: 'assistant-message-late-after-stop',
    delta: ' resurrected'
  }));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(late.applied, true);
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}:${message.status}`),
    ['user:cancel this:final', 'assistant:partial:cancelled']
  );
  assert.equal(selectSessionBusy(projection, 'session-1'), false);
  assert.equal(projection.sessions['session-1'].messages.length, 2);
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

test('legacy reconcile folds completed optimistic round without duplicating user or assistant bubbles', () => {
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(
    projection,
    buildCanonicalClientMessageSubmittedEvent({
      sessionId: 'session-1',
      content: 'debug question',
      clientMessageId: 'local-user:session-1:1000',
      createdAt: '2026-04-30T02:14:06.000Z',
      userTurnId: 'user-turn:session-1:round:1'
    })
  );
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-1',
    event_seq: 1,
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:1',
    message_id: 'assistant-message:model-turn:session-1:user:1:model:1',
    delta: 'debug'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_final',
    event_id: 'evt-2',
    event_seq: 2,
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:1',
    message_id: 'assistant-message:model-turn:session-1:user:1:model:1',
    content: 'debug answer'
  }));

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [
      {
        role: 'user',
        content: 'debug question',
        created_at: '2026-04-30T02:14:06.000Z'
      },
      {
        role: 'assistant',
        content: 'debug answer',
        created_at: '2026-04-30T02:14:07.000Z'
      }
    ],
    loading: false,
    running: false
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['user:debug question', 'assistant:debug answer']
  );
  assert.deepEqual(
    visible.map((message) => message.id),
    ['local-user:session-1:1000', 'assistant-message:model-turn:session-1:user:1:model:1']
  );
  assert.equal(projection.sessions['session-1'].messages.length, 2);
});

test('legacy reconcile keeps synthetic greeting out of runtime projection while preserving live turn order', () => {
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(
    projection,
    buildCanonicalClientMessageSubmittedEvent({
      sessionId: 'session-1',
      content: 'hello',
      clientMessageId: 'local-user:session-1:2000',
      createdAt: '2026-04-30T02:14:06.000Z',
      userTurnId: 'user-turn:session-1:round:1'
    })
  );
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_message_created',
    event_id: 'evt-local-assistant',
    event_seq: 1,
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:1',
    message_id: 'local-assistant:model-turn:session-1:user:1:model:1'
  }));

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [
      {
        role: 'assistant',
        content: 'synthetic greeting',
        isGreeting: true,
        created_at: '2026-04-30T02:14:01.000Z'
      },
      {
        role: 'user',
        content: 'hello',
        stream_round: 1,
        created_at: '2026-04-30T02:14:06.000Z'
      },
      {
        role: 'assistant',
        content: '',
        stream_round: 1,
        stream_incomplete: true,
        workflowStreaming: true,
        created_at: '2026-04-30T02:14:07.000Z'
      }
    ],
    loading: true,
    running: true
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['user:hello', 'assistant:']
  );
  assert.equal(visible.some((message) => message.raw?.isGreeting === true), false);
  assert.equal(projection.sessions['session-1'].messages.length, 2);
});

test('assistant created confirmation reuses an existing local assistant placeholder for the same model turn', () => {
  const projection = createChatRuntimeProjection();
  const userTurnId = 'user-turn:session-1:round:1';
  const modelTurnId = 'model-turn:session-1:user:1:model:1';
  const localAssistantId = `local-assistant:${modelTurnId}`;

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'user_message_created',
    event_id: 'evt-user-placeholder',
    event_seq: 1,
    user_turn_id: userTurnId,
    message_id: 'user-message-1',
    content: 'hello'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_message_created',
    event_id: 'evt-local-placeholder',
    event_seq: 2,
    user_turn_id: userTurnId,
    model_turn_id: modelTurnId,
    message_id: localAssistantId
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_message_created',
    event_id: 'evt-server-confirm',
    event_seq: 3,
    user_turn_id: userTurnId,
    model_turn_id: modelTurnId,
    message_id: 'server-assistant-message-1'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-server-delta',
    event_seq: 4,
    user_turn_id: userTurnId,
    model_turn_id: modelTurnId,
    message_id: 'server-assistant-message-1',
    delta: 'answer'
  }));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['user:hello', 'assistant:answer']
  );
  assert.equal(visible[1].id, localAssistantId);
  assert.equal(
    visible.filter((message) => message.role === 'assistant').length,
    1
  );
  assert.equal(projection.sessions['session-1'].messages.length, 2);
});

test('legacy reconcile keeps historical assistant separate from later completed answer after refresh', () => {
  const projection = createChatRuntimeProjection();
  const greeting = {
    role: 'assistant',
    content: 'ready prompt',
    created_at: '2026-04-30T02:14:01.000Z'
  };

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [greeting],
    loading: false,
    running: false
  });
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'user_message_created',
    event_id: 'evt-user',
    event_seq: 1,
    user_turn_id: 'user-turn:session-1:round:1',
    message_id: 'user-message-1',
    content: 'question',
    created_at: '2026-04-30T02:14:06.000Z'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-assistant-1',
    event_seq: 2,
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:1',
    message_id: 'assistant-message:model-turn:session-1:user:1:model:1',
    reasoning_delta: 'thinking',
    created_at: '2026-04-30T02:14:07.000Z'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_final',
    event_id: 'evt-assistant-2',
    event_seq: 3,
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:1',
    message_id: 'assistant-message:model-turn:session-1:user:1:model:1',
    content: 'answer',
    created_at: '2026-04-30T02:14:08.000Z'
  }));

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [
      greeting,
      {
        role: 'user',
        content: 'question',
        created_at: '2026-04-30T02:14:06.000Z'
      },
      {
        role: 'assistant',
        content: 'answer',
        reasoning: 'thinking',
        created_at: '2026-04-30T02:14:08.000Z'
      }
    ],
    loading: false,
    running: false
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}:${message.reasoning}`),
    ['assistant:ready prompt:', 'user:question:', 'assistant:answer:thinking']
  );
  assert.equal(visible[0].modelTurnId.startsWith('legacy-model-turn:'), true);
  assert.equal(visible[2].id, 'assistant-message:model-turn:session-1:user:1:model:1');
  assert.equal(projection.sessions['session-1'].messages.length, 3);
});

test('authoritative legacy reconcile prunes event snapshot residues and preserves chronological refresh order', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-live-1',
    event_seq: 1,
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:1',
    message_id: 'assistant-message:model-turn:session-1:user:1:model:1',
    delta: 'hello'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_final',
    event_id: 'evt-live-2',
    event_seq: 2,
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:1',
    message_id: 'assistant-message:model-turn:session-1:user:1:model:1',
    content: 'hello world'
  }));

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    authoritative: true,
    messages: [
      {
        role: 'assistant',
        content: 'greeting',
        created_at: '2026-04-30T02:14:01.000Z'
      },
      {
        role: 'user',
        content: 'question one',
        created_at: '2026-04-30T02:14:06.000Z'
      },
      {
        role: 'assistant',
        content: 'answer one',
        created_at: '2026-04-30T02:14:08.000Z'
      },
      {
        role: 'user',
        content: 'question two',
        created_at: '2026-04-30T02:14:16.000Z'
      },
      {
        role: 'assistant',
        content: 'answer two',
        created_at: '2026-04-30T02:14:18.000Z'
      }
    ],
    loading: false,
    running: false
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['assistant:greeting', 'user:question one', 'assistant:answer one', 'user:question two', 'assistant:answer two']
  );
  assert.equal(visible.length, 5);
  assert.equal(projection.sessions['session-1'].messages.length, 5);
  assert.equal(Object.keys(projection.sessions['session-1'].messageById).length, 5);
});

test('authoritative legacy reconcile reorders same-count refresh snapshots to legacy order', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-live-1',
    event_seq: 1,
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:1',
    message_id: 'assistant-message:model-turn:session-1:user:1:model:1',
    delta: 'live'
  }));

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    authoritative: true,
    messages: [
      {
        role: 'assistant',
        content: 'greeting',
        created_at: '2026-04-30T02:14:01.000Z'
      },
      {
        role: 'user',
        content: 'question',
        created_at: '2026-04-30T02:14:06.000Z'
      },
      {
        role: 'assistant',
        content: 'answer',
        created_at: '2026-04-30T02:14:08.000Z'
      }
    ],
    loading: false,
    running: false
  });

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    authoritative: true,
    messages: [
      {
        role: 'user',
        content: 'question',
        created_at: '2026-04-30T02:14:06.000Z'
      },
      {
        role: 'assistant',
        content: 'answer',
        created_at: '2026-04-30T02:14:08.000Z'
      },
      {
        role: 'assistant',
        content: 'greeting',
        created_at: '2026-04-30T02:14:01.000Z'
      }
    ],
    loading: false,
    running: false
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['assistant:greeting', 'user:question', 'assistant:answer']
  );
  assert.equal(projection.sessions['session-1'].messages.length, 3);
});

test('legacy reconcile replaces repeated full assistant snapshots without inflating content', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'user_message_created',
    event_id: 'evt-user',
    event_seq: 1,
    user_turn_id: 'user-turn:session-1:round:1',
    message_id: 'user-message-1',
    content: 'question',
    created_at: '2026-04-30T02:14:06.000Z'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-assistant-delta',
    event_seq: 2,
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:1',
    message_id: 'assistant-message:model-turn:session-1:user:1:model:1',
    delta: 'partial',
    created_at: '2026-04-30T02:14:07.000Z'
  }));

  const reconcile = (content: string, reasoning: string) => {
    applyChatRuntimeEvent(projection, {
      event_type: 'legacy_messages_reconciled',
      source: 'legacy',
      strict: false,
      session_id: 'session-1',
      messages: [
        {
          role: 'user',
          content: 'question',
          created_at: '2026-04-30T02:14:06.000Z'
        },
        {
          role: 'assistant',
          content,
          reasoning,
          created_at: '2026-04-30T02:14:08.000Z'
        }
      ],
      loading: false,
      running: false
    });
  };

  reconcile('full answer', 'think');
  reconcile('full answer', 'think');
  reconcile('full answer with tail', 'think more');

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}:${message.reasoning}`),
    ['user:question:', 'assistant:full answer with tail:think more']
  );
  assert.equal(visible[1].content.length, 'full answer with tail'.length);
  assert.equal(projection.sessions['session-1'].messages.length, 2);
});

test('late historical greeting keeps chronological order before an existing user turn', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'user_message_created',
    event_id: 'evt-user',
    event_seq: 1,
    user_turn_id: 'user-turn:session-1:round:1',
    message_id: 'user-message-1',
    content: 'question',
    created_at: '2026-04-30T02:14:06.000Z'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_final',
    event_id: 'evt-assistant',
    event_seq: 2,
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:1',
    message_id: 'assistant-message:model-turn:session-1:user:1:model:1',
    content: 'answer',
    created_at: '2026-04-30T02:14:08.000Z'
  }));

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [
      {
        role: 'assistant',
        content: 'ready prompt',
        created_at: '2026-04-30T02:14:01.000Z'
      },
      {
        role: 'user',
        content: 'question',
        created_at: '2026-04-30T02:14:06.000Z'
      },
      {
        role: 'assistant',
        content: 'answer',
        created_at: '2026-04-30T02:14:08.000Z'
      }
    ],
    loading: false,
    running: false
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['assistant:ready prompt', 'user:question', 'assistant:answer']
  );
  assert.equal(projection.sessions['session-1'].messages.length, 3);
});

test('cancelled user turn does not absorb a later same-text user turn after refresh', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(
    projection,
    buildCanonicalClientMessageSubmittedEvent({
      sessionId: 'session-1',
      content: 'continue',
      clientMessageId: 'local-user:first',
      createdAt: '2026-04-30T02:14:06.000Z',
      userTurnId: 'user-turn:session-1:round:1'
    })
  );
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-first-delta',
    event_seq: 1,
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:1',
    message_id: 'assistant-message:first',
    delta: 'partial',
    created_at: '2026-04-30T02:14:07.000Z'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'turn_cancelled',
    event_id: 'evt-first-cancel',
    event_seq: 2,
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:1',
    created_at: '2026-04-30T02:14:08.000Z'
  }));

  applyChatRuntimeEvent(
    projection,
    buildCanonicalClientMessageSubmittedEvent({
      sessionId: 'session-1',
      content: 'continue',
      clientMessageId: 'local-user:second',
      createdAt: '2026-04-30T02:14:16.000Z',
      userTurnId: 'user-turn:session-1:round:2'
    })
  );
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_final',
    event_id: 'evt-second-final',
    event_seq: 3,
    user_turn_id: 'user-turn:session-1:round:2',
    model_turn_id: 'model-turn:session-1:user:2:model:1',
    message_id: 'assistant-message:second',
    content: 'second answer',
    created_at: '2026-04-30T02:14:18.000Z'
  }));

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    authoritative: true,
    messages: [
      {
        message_id: 'history:1',
        role: 'user',
        content: 'continue',
        created_at: '2026-04-30T02:14:06.000Z'
      },
      {
        message_id: 'history:2',
        role: 'assistant',
        content: 'stopped',
        status: 'cancelled',
        cancelled: true,
        stop_reason: 'user_stop',
        created_at: '2026-04-30T02:14:08.000Z'
      },
      {
        message_id: 'history:3',
        role: 'user',
        content: 'continue',
        created_at: '2026-04-30T02:14:16.000Z'
      },
      {
        message_id: 'history:4',
        role: 'assistant',
        content: 'second answer',
        created_at: '2026-04-30T02:14:18.000Z'
      }
    ],
    loading: false,
    running: false
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}:${message.status}`),
    [
      'user:continue:final',
      'assistant:stopped:cancelled',
      'user:continue:final',
      'assistant:second answer:final'
    ]
  );
  assert.equal(visible[0].userTurnId, 'user-turn:session-1:round:1');
  assert.equal(visible[2].userTurnId, 'user-turn:session-1:round:2');
  assert.notEqual(visible[0].id, visible[2].id);
  assert.equal(projection.sessions['session-1'].messages.length, 4);
});

test('canonical stream adapter maps deltas and final events into one stable assistant message', () => {
  const projection = createChatRuntimeProjection();
  const streamEvents = [
    ...buildCanonicalChatRuntimeEvents({
      sessionId: 'session-1',
      eventType: 'llm_output_delta',
      eventId: 1,
      requestId: 'req-1',
      payload: {
        data: {
          delta: 'hello',
          reasoning_delta: 'thinking',
          model_round: 1
        }
      }
    }),
    ...buildCanonicalChatRuntimeEvents({
      sessionId: 'session-1',
      eventType: 'llm_output_delta',
      eventId: 2,
      requestId: 'req-1',
      payload: {
        data: {
          delta: ' world',
          model_round: 1
        }
      }
    }),
    ...buildCanonicalChatRuntimeEvents({
      sessionId: 'session-1',
      eventType: 'final',
      eventId: 3,
      requestId: 'req-1',
      payload: {
        data: {
          answer: 'hello world',
          model_round: 1
        }
      }
    })
  ];

  streamEvents.forEach((event) => applyChatRuntimeEvent(projection, event));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible.length, 1);
  assert.equal(visible[0].role, 'assistant');
  assert.equal(visible[0].content, 'hello world');
  assert.equal(visible[0].reasoning, 'thinking');
  assert.equal(visible[0].final, true);
  assert.equal(selectSessionBusy(projection, 'session-1'), false);
});

test('runtime reducer folds unstable delta message ids into one model turn bubble', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-1',
    event_seq: 1,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-fragment-1',
    delta: 'L'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-2',
    event_seq: 2,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-fragment-2',
    delta: 'e'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_final',
    event_id: 'evt-3',
    event_seq: 3,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-fragment-3',
    content: 'Let'
  }));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible.length, 1);
  assert.equal(visible[0].id, 'am-fragment-1');
  assert.equal(visible[0].content, 'Let');
  assert.equal(visible[0].final, true);
  assert.deepEqual(projection.sessions['session-1'].modelTurnById['mt-1'].messageIds, ['am-fragment-1']);
});

test('legacy reconcile folds streamed assistant fragments from one round into one bubble', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [
      {
        role: 'user',
        content: 'question',
        stream_round: 1
      },
      {
        role: 'assistant',
        content: 'L',
        stream_round: 1,
        stream_event_id: 1,
        stream_incomplete: true
      },
      {
        role: 'assistant',
        content: 'e',
        stream_round: 1,
        stream_event_id: 2,
        stream_incomplete: true
      },
      {
        role: 'assistant',
        content: 'Let',
        stream_round: 1,
        stream_event_id: 3,
        stream_incomplete: true
      }
    ],
    loading: true,
    running: true
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['user:question', 'assistant:Let']
  );
  assert.equal(visible[1].status, 'streaming');
  assert.equal(projection.sessions['session-1'].messages.length, 2);
});

test('legacy reconcile does not reuse a stale stream_round across later user turns', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [
      {
        role: 'user',
        content: 'first question',
        created_at: '2026-04-30T02:14:06.000Z'
      },
      {
        role: 'assistant',
        content: 'first answer',
        stream_round: 1,
        stream_event_id: 1,
        created_at: '2026-04-30T02:14:07.000Z'
      },
      {
        role: 'user',
        content: 'second question',
        created_at: '2026-04-30T02:14:16.000Z'
      },
      {
        role: 'assistant',
        content: 'second answer',
        stream_round: 1,
        stream_event_id: 2,
        created_at: '2026-04-30T02:14:18.000Z'
      }
    ],
    loading: false,
    running: false
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['user:first question', 'assistant:first answer', 'user:second question', 'assistant:second answer']
  );
  assert.notEqual(visible[1].modelTurnId, visible[3].modelTurnId);
  assert.equal(projection.sessions['session-1'].messages.length, 4);
});

test('legacy reconcile keeps two same-round assistant replies separate across a refreshed stop-and-continue flow', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [
      {
        role: 'assistant',
        content: 'greeting',
        created_at: '2026-04-30T02:14:01.000Z'
      },
      {
        role: 'user',
        content: 'first',
        created_at: '2026-04-30T02:14:06.000Z'
      },
      {
        role: 'assistant',
        content: 'first answer',
        reasoning: 'thinking one',
        stream_round: 1,
        stream_event_id: 11,
        created_at: '2026-04-30T02:14:07.000Z'
      },
      {
        role: 'user',
        content: 'second',
        created_at: '2026-04-30T02:14:16.000Z'
      },
      {
        role: 'assistant',
        content: 'second answer',
        reasoning: 'thinking two',
        stream_round: 1,
        stream_event_id: 12,
        created_at: '2026-04-30T02:14:18.000Z'
      }
    ],
    loading: false,
    running: false
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['assistant:greeting', 'user:first', 'assistant:first answer', 'user:second', 'assistant:second answer']
  );
  assert.notEqual(visible[2].modelTurnId, visible[4].modelTurnId);
  assert.equal(projection.sessions['session-1'].messages.length, 5);
});

test('canonical transcript snapshot rebuilds refresh order from backend turn indexes', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    payload: {
      transcript: [
        {
          role: 'assistant',
          content: 'greeting',
          message_id: 'history:1',
          user_turn_id: 'user-turn:session-1:round:0',
          model_turn_id: 'model-turn:session-1:user:0:model:1',
          turn_index: 1,
          created_at: '2026-04-30T02:14:01.000Z'
        },
        {
          role: 'user',
          content: 'first',
          message_id: 'history:2',
          user_turn_id: 'user-turn:session-1:round:1',
          turn_index: 2,
          created_at: '2026-04-30T02:14:06.000Z'
        },
        {
          role: 'assistant',
          content: 'cancelled',
          message_id: 'history:3',
          user_turn_id: 'user-turn:session-1:round:1',
          model_turn_id: 'model-turn:session-1:user:1:model:1',
          turn_index: 3,
          stream_round: 1,
          status: 'cancelled',
          cancelled: true,
          stop_reason: 'user_stop',
          created_at: '2026-04-30T02:14:07.000Z'
        },
        {
          role: 'user',
          content: 'second',
          message_id: 'history:4',
          user_turn_id: 'user-turn:session-1:round:2',
          turn_index: 4,
          created_at: '2026-04-30T02:14:16.000Z'
        },
        {
          role: 'assistant',
          content: 'second answer',
          message_id: 'history:5',
          user_turn_id: 'user-turn:session-1:round:2',
          model_turn_id: 'model-turn:session-1:user:2:model:1',
          turn_index: 5,
          stream_round: 1,
          created_at: '2026-04-30T02:14:18.000Z'
        }
      ]
    },
    loading: false,
    running: false
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['assistant:greeting', 'user:first', 'assistant:cancelled', 'user:second', 'assistant:second answer']
  );
  assert.equal(visible[2].cancelled, true);
  assert.notEqual(visible[2].modelTurnId, visible[4].modelTurnId);
  assert.deepEqual(
    projection.sessions['session-1'].messages,
    ['history:1', 'history:2', 'history:3', 'history:4', 'history:5']
  );
});


test('legacy reconcile overlaps text fragments without duplicating shared spans', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'user_message_created',
    event_id: 'evt-user',
    event_seq: 1,
    user_turn_id: 'user-turn:session-1:round:1',
    message_id: 'user-message-1',
    content: 'question'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-assistant-1',
    event_seq: 2,
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:1',
    message_id: 'assistant-message:model-turn:session-1:user:1:model:1',
    delta: 'abcde'
  }));

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [
      {
        role: 'user',
        content: 'question',
        stream_round: 1
      },
      {
        role: 'assistant',
        content: 'cdefg',
        stream_round: 1,
        stream_event_id: 10,
        stream_incomplete: true
      }
    ],
    loading: true,
    running: true
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['user:question', 'assistant:abcdefg']
  );
  assert.equal(projection.sessions['session-1'].messages.length, 2);
});

test('canonical stream adapter keeps queued request running until terminal event', () => {
  const projection = createChatRuntimeProjection();
  const queued = buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'queued',
    eventId: 4,
    requestId: 'req-queued',
    payload: { data: { queued: true } }
  });
  queued.forEach((event) => applyChatRuntimeEvent(projection, event));

  assert.equal(selectSessionBusy(projection, 'session-1'), true);
  assert.equal(selectSessionRuntimeStatus(projection, 'session-1'), 'queued');

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'turn_terminal',
    eventId: 5,
    requestId: 'req-queued',
    payload: { data: { status: 'completed' } }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  assert.equal(selectSessionBusy(projection, 'session-1'), false);
});

test('canonical client submit event materializes the local user turn', () => {
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(
    projection,
    buildCanonicalClientMessageSubmittedEvent({
      sessionId: 'session-1',
      content: 'hello',
      clientMessageId: 'client-message-1',
      createdAt: '2026-04-30T02:14:06.000Z'
    })
  );

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible.length, 1);
  assert.equal(visible[0].role, 'user');
  assert.equal(visible[0].content, 'hello');
  assert.equal(selectSessionBusy(projection, 'session-1'), true);
});

test('local client submit can materialize one assistant placeholder for projection render', () => {
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(
    projection,
    buildCanonicalClientMessageSubmittedEvent({
      sessionId: 'session-1',
      content: 'hello',
      clientMessageId: 'client-message-1',
      createdAt: '2026-04-30T02:14:06.000Z',
      userTurnId: 'user-turn-1'
    })
  );
  applyChatRuntimeEvent(projection, {
    event_type: 'assistant_message_created',
    source: 'local',
    strict: false,
    session_id: 'session-1',
    event_id: 'local:assistant-placeholder-1',
    user_turn_id: 'user-turn-1',
    model_turn_id: 'model-turn-1',
    message_id: 'assistant-placeholder-1',
    created_at: '2026-04-30T02:14:06.000Z'
  });
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_message_created',
    event_id: 'evt-assistant-created',
    event_seq: 1,
    user_turn_id: 'user-turn-1',
    model_turn_id: 'model-turn-1',
    message_id: 'server-assistant-1'
  }));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.status}:${message.id}`),
    ['user:final:client-message-1', 'assistant:waiting_first_output:assistant-placeholder-1']
  );
  assert.equal(projection.sessions['session-1'].messages.length, 2);
});

test('chat runtime reducer projects tool workflow lifecycle onto assistant message', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'tool_call_started',
    event_id: 'evt-tool-1',
    event_seq: 1,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    payload: {
      source_event_type: 'tool_call',
      data: {
        tool_call_id: 'call-1',
        tool: 'lookup',
        input: 'status'
      }
    }
  }));

  let visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible.length, 1);
  assert.equal(visible[0].status, 'tooling');
  assert.equal(visible[0].workflowItems?.length, 1);
  assert.equal(visible[0].workflowItems?.[0]?.status, 'loading');
  assert.equal(visible[0].workflowItems?.[0]?.eventType, 'tool_call');
  assert.equal(visible[0].workflowItems?.[0]?.toolCallId, 'call-1');
  assert.equal(selectSessionBusy(projection, 'session-1'), true);

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'tool_call_completed',
    event_id: 'evt-tool-2',
    event_seq: 2,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    payload: {
      source_event_type: 'tool_result',
      data: {
        tool_call_id: 'call-1',
        tool: 'lookup',
        output: 'ok'
      }
    }
  }));

  visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible[0].status, 'streaming');
  assert.equal(visible[0].workflowItems?.length, 1);
  assert.equal(visible[0].workflowItems?.[0]?.status, 'completed');
  assert.equal(visible[0].workflowItems?.[0]?.eventType, 'tool_result');
  assert.equal(visible[0].workflowItems?.[0]?.toolCallId, 'call-1');
});

test('chat runtime reducer keeps failed tool workflow detail terminal', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'tool_call_started',
    event_id: 'evt-tool-1',
    event_seq: 1,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    payload: {
      source_event_type: 'tool_call',
      data: {
        tool_call_id: 'call-1',
        tool: 'lookup'
      }
    }
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'tool_call_failed',
    event_id: 'evt-tool-2',
    event_seq: 2,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    payload: {
      source_event_type: 'tool_result',
      data: {
        tool_call_id: 'call-1',
        tool: 'lookup',
        error: 'failed'
      }
    }
  }));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible[0].status, 'failed');
  assert.equal(visible[0].workflowItems?.length, 1);
  assert.equal(visible[0].workflowItems?.[0]?.status, 'failed');
  assert.equal(visible[0].workflowItems?.[0]?.eventType, 'tool_result');
  assert.equal(selectSessionBusy(projection, 'session-1'), false);
});

test('approval request and result keep one workflow item and explicit waiting status', () => {
  const projection = createChatRuntimeProjection();

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'approval_request',
    eventId: 1,
    requestId: 'req-approval',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        approval_id: 'approval-1',
        tool: 'edit',
        summary: 'needs approval'
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  let visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(selectSessionBusy(projection, 'session-1'), true);
  assert.equal(selectSessionRuntimeStatus(projection, 'session-1'), 'waiting_approval');
  assert.equal(visible[0].status, 'tooling');
  assert.equal(visible[0].workflowItems?.length, 1);
  assert.equal(visible[0].workflowItems?.[0]?.eventType, 'approval_request');
  assert.equal(visible[0].workflowItems?.[0]?.status, 'loading');
  assert.equal(visible[0].workflowItems?.[0]?.approvalId, 'approval-1');

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'approval_result',
    eventId: 2,
    requestId: 'req-approval',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        approval_id: 'approval-1',
        tool: 'edit',
        decision: 'approve_once',
        status: 'completed'
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(selectSessionBusy(projection, 'session-1'), true);
  assert.equal(selectSessionRuntimeStatus(projection, 'session-1'), 'running');
  assert.equal(visible[0].status, 'streaming');
  assert.equal(visible[0].workflowItems?.length, 1);
  assert.equal(visible[0].workflowItems?.[0]?.eventType, 'approval_result');
  assert.equal(visible[0].workflowItems?.[0]?.status, 'completed');
  assert.equal(visible[0].workflowItems?.[0]?.approvalId, 'approval-1');
});

test('canonical subagent workflow events update one projected assistant and subagent card', () => {
  const projection = createChatRuntimeProjection();
  const running = buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'subagent_dispatch_item_update',
    eventId: 1,
    requestId: 'req-subagent',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        session_id: 'child-session-1',
        run_id: 'child-run-1',
        label: 'Worker A',
        status: 'running',
        summary: 'started'
      }
    }
  });

  assert.equal(running.length, 1);
  assert.equal(running[0].event_type, 'workflow_event');
  running.forEach((event) => applyChatRuntimeEvent(projection, event));

  let visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible.length, 1);
  assert.equal(visible[0].role, 'assistant');
  assert.equal(visible[0].status, 'tooling');
  assert.equal(visible[0].workflowItems?.length, 1);
  assert.equal(visible[0].workflowItems?.[0]?.kind, 'subagent');
  assert.equal(visible[0].workflowItems?.[0]?.status, 'loading');
  assert.equal(visible[0].subagents?.length, 1);
  assert.equal(visible[0].subagents?.[0]?.key, 'child-run-1');
  assert.equal(visible[0].subagents?.[0]?.status, 'running');
  assert.equal(visible[0].subagents?.[0]?.terminal, false);
  assert.equal(selectSessionBusy(projection, 'session-1'), true);

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'subagent_dispatch_finish',
    eventId: 2,
    requestId: 'req-subagent',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        session_id: 'child-session-1',
        run_id: 'child-run-1',
        label: 'Worker A',
        status: 'completed',
        result: 'done'
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible.length, 1);
  assert.equal(visible[0].status, 'streaming');
  assert.equal(visible[0].workflowItems?.length, 1);
  assert.equal(visible[0].workflowItems?.[0]?.status, 'completed');
  assert.equal(visible[0].subagents?.length, 1);
  assert.equal(visible[0].subagents?.[0]?.key, 'child-run-1');
  assert.equal(visible[0].subagents?.[0]?.status, 'completed');
  assert.equal(visible[0].subagents?.[0]?.terminal, true);
  assert.equal(visible[0].subagents?.[0]?.failed, false);
  assert.equal(selectSessionBusy(projection, 'session-1'), true);

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'turn_completed',
    event_id: 'evt-subagent-terminal',
    event_seq: 3,
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:1'
  }));

  visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible[0].status, 'final');
  assert.equal(visible[0].workflowItems?.[0]?.status, 'completed');
  assert.equal(visible[0].subagents?.[0]?.status, 'completed');
  assert.equal(selectSessionBusy(projection, 'session-1'), false);
});

test('team workflow events update one projected workflow item by task identity', () => {
  const projection = createChatRuntimeProjection();

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'team_task_update',
    eventId: 1,
    requestId: 'req-team',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        task_id: 'task-1',
        title: 'Research',
        status: 'running'
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));
  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'team_task_result',
    eventId: 2,
    requestId: 'req-team',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        task_id: 'task-1',
        title: 'Research',
        status: 'completed'
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible.length, 1);
  assert.equal(visible[0].workflowItems?.length, 1);
  assert.equal(visible[0].workflowItems?.[0]?.kind, 'team');
  assert.equal(visible[0].workflowItems?.[0]?.eventType, 'team_task_result');
  assert.equal(visible[0].workflowItems?.[0]?.status, 'completed');
  assert.equal(visible[0].workflowItems?.[0]?.taskId, 'task-1');
});

test('terminal turn settles projected subagents even without workflow items', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [
      {
        message_id: 'message-assistant-1',
        role: 'assistant',
        content: '',
        workflowStreaming: true,
        subagents: [
          {
            key: 'child-run-1',
            run_id: 'child-run-1',
            status: 'running',
            terminal: false,
            canTerminate: true
          }
        ]
      }
    ],
    loading: true,
    running: true
  });

  applyChatRuntimeEvent(projection, {
    event_type: 'turn_completed',
    source: 'test',
    strict: true,
    session_id: 'session-1',
    event_id: 'event-2',
    event_seq: 2,
    user_turn_id: 'legacy-user-turn:orphan:0',
    model_turn_id: 'legacy-model-turn:message-assistant-1'
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible[0].subagents?.[0]?.status, 'completed');
  assert.equal(visible[0].subagents?.[0]?.terminal, true);
  assert.equal(visible[0].subagents?.[0]?.canTerminate, false);
  assert.equal(selectSessionBusy(projection, 'session-1'), false);
});

test('legacy reconcile treats active subagents as tooling even without workflow flags', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [
      {
        role: 'assistant',
        content: '',
        subagents: [
          {
            key: 'child-run-1',
            run_id: 'child-run-1',
            status: 'running',
            terminal: false
          }
        ]
      }
    ],
    loading: false,
    running: false
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible[0].status, 'tooling');
  assert.equal(selectSessionBusy(projection, 'session-1'), true);
});

test('subagent close without status does not leave projection stuck in tooling', () => {
  const projection = createChatRuntimeProjection();

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'subagent_dispatch_item_update',
    eventId: 1,
    requestId: 'req-subagent-close',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        session_id: 'child-session-1',
        run_id: 'child-run-1',
        status: 'running'
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));
  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'subagent_close',
    eventId: 2,
    requestId: 'req-subagent-close',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        session_id: 'child-session-1',
        run_id: 'child-run-1'
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible[0].status, 'streaming');
  assert.equal(visible[0].workflowItems?.[0]?.status, 'completed');
  assert.equal(visible[0].subagents?.[0]?.status, 'completed');
  assert.equal(visible[0].subagents?.[0]?.terminal, true);
});

test('canonical snapshot bridge replays raw stream events with event_seq', () => {
  const projection = createChatRuntimeProjection();
  buildCanonicalSessionEventsSnapshot({
    sessionId: 'session-1',
    payload: {
      last_event_id: 3,
      running: false,
      events: [
        {
          event: 'llm_output_delta',
          event_id: 1,
          event_seq: 1,
          data: {
            data: {
              delta: 'snap',
              user_round: 1,
              model_round: 1
            }
          }
        },
        {
          event: 'final',
          event_id: 2,
          event_seq: 2,
          data: {
            data: {
              answer: 'snapshot answer',
              user_round: 1,
              model_round: 1
            }
          }
        },
        {
          event: 'turn_terminal',
          event_id: 3,
          event_seq: 3,
          data: {
            data: {
              status: 'completed',
              user_round: 1,
              model_round: 1
            }
          }
        }
      ]
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible.length, 1);
  assert.equal(visible[0].role, 'assistant');
  assert.equal(visible[0].content, 'snapshot answer');
  assert.equal(visible[0].final, true);
  assert.equal(selectSessionBusy(projection, 'session-1'), false);
});

test('canonical error followed by turn terminal keeps one failed assistant bubble', () => {
  const projection = createChatRuntimeProjection();
  const failure = 'LLM stream request failed: invalid tool name';

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'error',
    eventId: 1,
    requestId: 'req-failed',
    payload: {
      data: {
        user_round: 3,
        model_round: 1,
        message: failure
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'turn_terminal',
    eventId: 2,
    requestId: 'req-failed',
    payload: {
      data: {
        user_round: 3,
        model_round: 1,
        status: 'failed',
        error: {
          message: failure
        }
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible.length, 1);
  assert.equal(visible[0].role, 'assistant');
  assert.equal(visible[0].status, 'failed');
  assert.equal(visible[0].content, failure);
  assert.equal(selectSessionRuntimeStatus(projection, 'session-1'), 'failed');
});

test('canonical snapshot bridge splits persisted delta segments by event id', () => {
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-1',
    event_seq: 1,
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:1',
    message_id: 'assistant-message:model-turn:session-1:user:1:model:1',
    delta: 'a'
  }));

  buildCanonicalSessionEventsSnapshot({
    sessionId: 'session-1',
    payload: {
      last_event_id: 3,
      running: true,
      events: [
        {
          event: 'llm_output_delta',
          event_id: 3,
          event_seq: 3,
          data: {
            data: {
              segments: [
                { event_id: 2, delta: 'b', user_round: 1, model_round: 1 },
                { event_id: 3, delta: 'c', user_round: 1, model_round: 1 }
              ]
            }
          }
        }
      ]
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible.length, 1);
  assert.equal(visible[0].content, 'abc');
  assert.equal(projection.sessions['session-1'].appliedSeq, 3);
});

test('strict runtime reducer buffers small event_seq gaps until missing deltas arrive', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-1',
    event_seq: 1,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    delta: 'A'
  }));
  const pending = applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-3',
    event_seq: 3,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    delta: 'C'
  }));

  let visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(pending.pending, true);
  assert.equal(pending.applied, false);
  assert.equal(visible[0].content, 'A');
  assert.equal(projection.sessions['session-1'].appliedSeq, 1);
  assert.equal(projection.sessions['session-1'].pendingSequentialEvents.length, 1);

  const drain = applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-2',
    event_seq: 2,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    delta: 'B'
  }));

  visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(drain.applied, true);
  assert.equal(drain.drained, 1);
  assert.equal(visible[0].content, 'ABC');
  assert.equal(projection.sessions['session-1'].appliedSeq, 3);
  assert.equal(projection.sessions['session-1'].pendingSequentialEvents.length, 0);
});

test('strict runtime reducer keeps final event behind buffered deltas', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-1',
    event_seq: 1,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    delta: 'hello'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_final',
    event_id: 'evt-3',
    event_seq: 3,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    content: 'hello world'
  }));

  let visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible[0].content, 'hello');
  assert.equal(visible[0].final, false);

  const drain = applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-2',
    event_seq: 2,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    delta: ' world'
  }));

  visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(drain.drained, 1);
  assert.equal(visible[0].content, 'hello world');
  assert.equal(visible[0].final, true);
  assert.equal(projection.sessions['session-1'].appliedSeq, 3);
});

test('strict runtime reducer asks for sync on large event_seq gaps instead of buffering indefinitely', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-1',
    event_seq: 1,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    delta: 'A'
  }));
  const result = applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-9',
    event_seq: 9,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    delta: 'I'
  }));

  const session = projection.sessions['session-1'];
  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(result.applied, true);
  assert.equal(result.pending, undefined);
  assert.equal(result.reason, 'event_seq_gap');
  assert.equal(session.syncRequired, true);
  assert.equal(session.pendingSequentialEvents.length, 0);
  assert.equal(session.appliedSeq, 9);
  assert.equal(visible[0].content, 'AI');
  assert.ok(session.invariantViolations.some((violation) => violation.code === 'event_seq_gap'));
});

test('runtime reducer folds legacy-confirmed user message into local optimistic user turn', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(
    projection,
    buildCanonicalClientMessageSubmittedEvent({
      sessionId: 'session-1',
      content: 'same user text',
      clientMessageId: 'local-user-1',
      createdAt: '2026-04-30T02:14:06.000Z',
      userTurnId: 'user-turn:session-1:round:1'
    })
  );
  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [
      {
        role: 'user',
        content: 'same user text',
        stream_round: 1,
        created_at: '2026-04-30T02:14:06.000Z'
      },
      {
        role: 'assistant',
        content: '',
        stream_round: 1,
        stream_incomplete: true,
        workflowStreaming: true,
        created_at: '2026-04-30T02:14:06.100Z'
      }
    ],
    loading: true,
    running: true
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['user:same user text', 'assistant:']
  );
  assert.equal(visible[0].id, 'local-user-1');
  assert.equal(projection.sessions['session-1'].messages.length, 2);
});

test('runtime reducer folds multiple model rounds and legacy fragments into one active assistant bubble', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'user_message_created',
    event_id: 'evt-user',
    event_seq: 1,
    user_turn_id: 'user-turn:session-1:round:1',
    message_id: 'user-message-1',
    content: 'question'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-model-1',
    event_seq: 2,
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:1',
    message_id: 'assistant-message:model-turn:session-1:user:1:model:1',
    reasoning_delta: 'think'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'workflow_event',
    event_id: 'evt-model-2',
    event_seq: 3,
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:2',
    message_id: 'assistant-message:model-turn:session-1:user:1:model:2',
    payload: {
      source_event_type: 'team_task_update',
      data: {
        task_id: 'task-1',
        status: 'running'
      }
    }
  }));
  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'session-1',
    messages: [
      {
        message_id: 'user-message-1',
        role: 'user',
        content: 'question',
        user_turn_id: 'user-turn:session-1:round:1',
        stream_round: 1
      },
      {
        role: 'assistant',
        content: '',
        reasoning: ' more',
        stream_event_id: 123,
        stream_round: 1,
        stream_incomplete: true
      },
      {
        role: 'assistant',
        content: 'answer',
        stream_event_id: 189,
        stream_round: 1,
        stream_incomplete: true
      }
    ],
    loading: true,
    running: true
  });
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 'evt-model-3',
    event_seq: 4,
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:3',
    message_id: 'assistant-message:model-turn:session-1:user:1:model:3',
    delta: '!'
  }));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => message.role),
    ['user', 'assistant']
  );
  assert.equal(visible[1].content, 'answer!');
  assert.equal(visible[1].reasoning, 'think more');
  assert.equal(visible[1].workflowItems?.length, 1);
  assert.equal(projection.sessions['session-1'].messages.length, 2);
});
