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
  selectRuntimeLastAppliedEventId,
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

test('local terminal events materialize assistant failure and cancellation text', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, {
    event_type: 'client_message_submitted',
    source: 'local',
    strict: false,
    session_id: 'session-1',
    event_id: 'local-submit-1',
    user_turn_id: 'ut-local-1',
    message_id: 'um-local-1',
    content: 'question'
  });
  applyChatRuntimeEvent(projection, {
    event_type: 'assistant_message_created',
    source: 'local',
    strict: false,
    session_id: 'session-1',
    event_id: 'local-assistant-1',
    user_turn_id: 'ut-local-1',
    model_turn_id: 'mt-local-1',
    message_id: 'am-local-1'
  });
  applyChatRuntimeEvent(projection, {
    event_type: 'turn_failed',
    source: 'local',
    strict: false,
    session_id: 'session-1',
    event_id: 'local-failed-1',
    user_turn_id: 'ut-local-1',
    model_turn_id: 'mt-local-1',
    message_id: 'am-local-1',
    content: 'request failed'
  });
  applyChatRuntimeEvent(projection, {
    event_type: 'turn_cancelled',
    source: 'local',
    strict: false,
    session_id: 'session-1',
    event_id: 'local-cancelled-1',
    user_turn_id: 'ut-local-2',
    model_turn_id: 'mt-local-2',
    message_id: 'am-local-2',
    content: 'request aborted'
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  const failed = visible.find((message) => message.id === 'am-local-1');
  const cancelled = visible.find((message) => message.id === 'am-local-2');
  assert.equal(failed?.status, 'failed');
  assert.equal(failed?.content, 'request failed');
  assert.equal(cancelled?.status, 'cancelled');
  assert.equal(cancelled?.content, 'request aborted');
  assert.equal(selectSessionBusy(projection, 'session-1'), false);
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
    strict: false,
    session_id: 'session-1',
    messages,
    loading: false,
    running: false
  });
  const firstVisible = selectVisibleMessageProjections(projection, 'session-1');
  const firstIds = firstVisible.map((message) => message.id);

  applyChatRuntimeEvent(projection, {
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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

test('tool result and final events with weak turn ids fold into the active tool assistant bubble', () => {
  const projection = createChatRuntimeProjection();
  const userTurnId = 'user-turn:session-1:round:1';
  const modelTurnId = 'model-turn:session-1:user:1:model:1';
  const toolAssistantId = 'assistant-message:tool-call-turn';

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'user_message_created',
    event_id: 'evt-user-tool',
    event_seq: 1,
    user_turn_id: userTurnId,
    message_id: 'user-message-tool',
    content: 'use a tool'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'tool_call_started',
    event_id: 'evt-tool-call',
    event_seq: 2,
    user_turn_id: userTurnId,
    model_turn_id: modelTurnId,
    message_id: toolAssistantId,
    payload: {
      source_event_type: 'tool_call',
      data: {
        tool: 'sample_tool',
        tool_call_id: 'call-1'
      }
    }
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'tool_call_completed',
    event_id: 'evt-tool-result',
    event_seq: 3,
    user_turn_id: 'user-turn:session-1:request:req-1',
    model_turn_id: 'model-turn:session-1:request:req-1',
    message_id: 'assistant-message:model-turn:session-1:request:req-1',
    payload: {
      source_event_type: 'tool_result',
      data: {
        tool: 'sample_tool',
        tool_call_id: 'call-1',
        status: 'completed'
      }
    }
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_final',
    event_id: 'evt-final-after-tool',
    event_seq: 4,
    user_turn_id: 'user-turn:session-1:request:req-1',
    model_turn_id: 'model-turn:session-1:request:req-1',
    message_id: 'assistant-message:model-turn:session-1:request:req-1',
    content: 'tool-backed answer'
  }));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['user:use a tool', 'assistant:tool-backed answer']
  );
  assert.equal(visible[1].id, toolAssistantId);
  assert.equal(
    visible.filter((message) => message.role === 'assistant').length,
    1
  );
  assert.equal(visible[1].workflowItems?.length, 1);
  assert.equal(
    projection.sessions['session-1'].messages.length,
    2
  );
});

test('chat runtime projection order follows event sequence instead of wall clock timestamps', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'user_message_created',
    event_id: 'evt-clock-1',
    event_seq: 1,
    user_turn_id: 'ut-clock-1',
    message_id: 'um-clock-1',
    content: 'first question',
    created_at: '2026-04-30T02:19:06.000Z'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_final',
    event_id: 'evt-clock-2',
    event_seq: 2,
    user_turn_id: 'ut-clock-1',
    model_turn_id: 'mt-clock-1',
    message_id: 'am-clock-1',
    content: 'first answer',
    created_at: '2026-04-30T02:19:07.000Z'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'user_message_created',
    event_id: 'evt-clock-3',
    event_seq: 3,
    user_turn_id: 'ut-clock-2',
    message_id: 'um-clock-2',
    content: 'second question',
    created_at: '2026-04-30T02:14:06.000Z'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_final',
    event_id: 'evt-clock-4',
    event_seq: 4,
    user_turn_id: 'ut-clock-2',
    model_turn_id: 'mt-clock-2',
    message_id: 'am-clock-2',
    content: 'second answer',
    created_at: '2026-04-30T02:14:07.000Z'
  }));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    [
      'user:first question',
      'assistant:first answer',
      'user:second question',
      'assistant:second answer'
    ]
  );
});

test('legacy reconcile keeps historical assistant separate from later completed answer after refresh', () => {
  const projection = createChatRuntimeProjection();
  const greeting = {
    role: 'assistant',
    content: 'ready prompt',
    created_at: '2026-04-30T02:14:01.000Z'
  };

  applyChatRuntimeEvent(projection, {
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    ['user:question:', 'assistant:answer:thinking', 'assistant:ready prompt:']
  );
  assert.equal(visible[1].id, 'assistant-message:model-turn:session-1:user:1:model:1');
  assert.equal(visible[2].modelTurnId.startsWith('legacy-model-turn:'), true);
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    ['user:question', 'assistant:answer', 'assistant:greeting']
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
      event_type: 'session_snapshot',
      source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    ['user:question', 'assistant:answer', 'assistant:ready prompt']
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
      eventType: 'think_delta',
      eventId: 1,
      requestId: 'req-1',
      payload: {
        data: {
          reasoning_delta: 'thinking',
          model_round: 1
        }
      }
    }),
    ...buildCanonicalChatRuntimeEvents({
      sessionId: 'session-1',
      eventType: 'reasoning_delta',
      eventId: 2,
      requestId: 'req-1',
      payload: {
        data: {
          think_delta: ' more',
          model_round: 1
        }
      }
    }),
    ...buildCanonicalChatRuntimeEvents({
      sessionId: 'session-1',
      eventType: 'llm_output_delta',
      eventId: 3,
      requestId: 'req-1',
      payload: {
        data: {
          delta: 'hello',
          model_round: 1
        }
      }
    }),
    ...buildCanonicalChatRuntimeEvents({
      sessionId: 'session-1',
      eventType: 'llm_output_delta',
      eventId: 4,
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
      eventId: 5,
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
  assert.equal(visible[0].reasoning, 'thinking more');
  assert.equal(visible[0].final, true);
  assert.equal(selectSessionBusy(projection, 'session-1'), false);
});

test('canonical llm_output snapshot updates assistant without legacy workflow processor', () => {
  const projection = createChatRuntimeProjection();

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'llm_output',
    eventId: 10,
    requestId: 'req-output',
    payload: {
      data: {
        content: 'draft answer',
        reasoning: 'draft reasoning',
        model_round: 1,
        usage: {
          input_tokens: 3,
          output_tokens: 5
        }
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  let visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible.length, 1);
  assert.equal(visible[0].content, 'draft answer');
  assert.equal(visible[0].reasoning, 'draft reasoning');
  assert.equal(visible[0].status, 'streaming');
  assert.equal(visible[0].final, false);
  assert.deepEqual(visible[0].display?.stats?.usage, { input: 3, output: 5, total: 8 });
  assert.equal(selectSessionBusy(projection, 'session-1'), true);

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'llm_output',
    eventId: 11,
    requestId: 'req-output',
    payload: {
      data: {
        content: 'final answer',
        stop_reason: 'stop',
        model_round: 1
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible.length, 1);
  assert.equal(visible[0].content, 'final answer');
  assert.equal(visible[0].status, 'final');
  assert.equal(visible[0].final, true);
});

test('canonical stream adapter projects unknown stream events as workflow items', () => {
  const projection = createChatRuntimeProjection();

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'custom_unknown_event',
    eventId: 12,
    requestId: 'req-unknown',
    payload: {
      data: {
        detail: 'kept visible'
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible.length, 1);
  assert.equal(visible[0].role, 'assistant');
  assert.equal(visible[0].status, 'tooling');
  assert.equal(visible[0].workflowItems?.[0]?.eventType, 'custom_unknown_event');
  assert.match(String(visible[0].workflowItems?.[0]?.detail || ''), /kept visible/);
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
  const visibleWhileQueued = selectVisibleMessageProjections(projection, 'session-1');
  const queuedAssistant = visibleWhileQueued.find((message) => message.role === 'assistant');
  assert.ok(queuedAssistant);
  assert.equal(queuedAssistant.status, 'tooling');
  assert.equal(queuedAssistant.workflowItems?.[0]?.id, 'queue:status');
  assert.equal(queuedAssistant.workflowItems?.[0]?.eventType, 'queued');
  assert.equal(queuedAssistant.workflowItems?.[0]?.status, 'pending');

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

test('canonical round start and channel user events materialize user messages', () => {
  const projection = createChatRuntimeProjection();

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'round_start',
    eventId: 10,
    requestId: 'req-round',
    payload: { data: { message: 'round question', user_round: 1 } }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));
  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'channel_message',
    eventId: 11,
    requestId: 'req-channel',
    payload: { data: { role: 'user', content: 'channel question', user_round: 2 } }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['user:round question', 'user:channel question']
  );
});

test('canonical channel assistant event materializes terminal assistant reply', () => {
  const projection = createChatRuntimeProjection();

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'channel_message',
    eventId: 12,
    requestId: 'req-channel',
    payload: { data: { role: 'assistant', content: 'channel answer', user_round: 1 } }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.equal(visible.length, 1);
  assert.equal(visible[0].role, 'assistant');
  assert.equal(visible[0].content, 'channel answer');
  assert.equal(visible[0].status, 'final');
  assert.equal(selectSessionBusy(projection, 'session-1'), false);
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

test('canonical tool output events project without legacy workflow processor state', () => {
  const projection = createChatRuntimeProjection();
  const events = buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'tool_output_delta',
    eventId: 21,
    requestId: 'req-tool-output',
    payload: {
      data: {
        tool_call_id: 'call-1',
        tool: 'execute_command',
        command_session_id: 'cmd-1',
        output: 'partial output'
      }
    }
  });

  assert.equal(events.length, 1);
  assert.equal(events[0].event_type, 'tool_call_delta');
  events.forEach((event) => applyChatRuntimeEvent(projection, event));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  const assistant = visible.find((message) => message.role === 'assistant');
  assert.ok(assistant);
  assert.equal(assistant.status, 'tooling');
  assert.equal(assistant.workflowItems?.length, 1);
  assert.equal(assistant.workflowItems?.[0]?.status, 'loading');
  assert.equal(assistant.workflowItems?.[0]?.eventType, 'tool_output_delta');
  assert.equal(assistant.workflowItems?.[0]?.toolCallId, 'call-1');
  assert.equal(assistant.workflowItems?.[0]?.commandSessionId, 'cmd-1');
});

test('canonical visible workflow events project retry slow-client and compaction state', () => {
  const projection = createChatRuntimeProjection();

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'llm_request',
    eventId: 30,
    requestId: 'req-workflow',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        payload_summary: { messages: 2 }
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'llm_stream_retry',
    eventId: 31,
    requestId: 'req-workflow',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        attempt: 2,
        max_attempts: 5,
        delay_s: 1.5,
        retry_reason: 'rate_limit',
        timestamp: '2026-04-30T02:14:07.000Z'
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  let visible = selectVisibleMessageProjections(projection, 'session-1');
  let assistant = visible.find((message) => message.role === 'assistant');
  assert.ok(assistant);
  assert.equal(assistant.workflowItems?.length, 2);
  assert.equal(assistant.workflowItems?.[0]?.eventType, 'llm_request');
  assert.equal(assistant.workflowItems?.[0]?.status, 'completed');
  assert.equal(assistant.workflowItems?.[1]?.eventType, 'llm_stream_retry');
  assert.equal(assistant.workflowItems?.[1]?.status, 'loading');
  assert.equal(assistant.workflowItems?.[1]?.attempt, 2);
  assert.equal(assistant.display?.retry_attempt, 2);
  assert.equal(assistant.display?.retry_max_attempts, 5);
  assert.equal(assistant.display?.retry_delay_s, 1.5);
  assert.equal(assistant.display?.retry_reason, 'rate_limit');
  assert.equal(assistant.display?.retry_started_at_ms, Date.parse('2026-04-30T02:14:07.000Z'));

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'progress',
    eventId: 32,
    requestId: 'req-workflow',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        round: 1,
        stage: 'context_guard',
        summary: 'guarding context'
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  visible = selectVisibleMessageProjections(projection, 'session-1');
  assistant = visible.find((message) => message.role === 'assistant');
  assert.ok(assistant);
  const compactionProgress = assistant.workflowItems?.find((item) => item.eventType === 'compaction_progress');
  assert.ok(compactionProgress);
  assert.equal(compactionProgress.status, 'loading');
  assert.equal(compactionProgress.toolName, 'context_compaction');
  assert.equal(assistant.display?.manual_compaction_marker, true);

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'slow_client',
    eventId: 33,
    requestId: 'req-workflow',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        reason: 'queue_full_resume_required',
        queue_capacity: 2
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  visible = selectVisibleMessageProjections(projection, 'session-1');
  assistant = visible.find((message) => message.role === 'assistant');
  assert.ok(assistant);
  const slowClient = assistant.workflowItems?.find((item) => item.eventType === 'slow_client');
  assert.ok(slowClient);
  assert.equal(slowClient.status, 'failed');
  assert.equal(assistant.status, 'final');
  assert.equal(assistant.failed, false);
  assert.equal(assistant.display?.slow_client, true);
  assert.equal(assistant.display?.resume_available, true);
  assert.equal(selectSessionBusy(projection, 'session-1'), false);
});

test('canonical command session events project without replacing command store side effects', () => {
  const projection = createChatRuntimeProjection();
  const events = buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'command_session_summary',
    eventId: 40,
    requestId: 'req-command',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        command_session_id: 'cmd-1',
        command: 'run',
        status: 'completed',
        stdout: 'ok'
      }
    }
  });

  assert.equal(events.length, 1);
  assert.equal(events[0].event_type, 'workflow_event');
  events.forEach((event) => applyChatRuntimeEvent(projection, event));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  const assistant = visible.find((message) => message.role === 'assistant');
  assert.ok(assistant);
  assert.equal(assistant.workflowItems?.length, 1);
  assert.equal(assistant.workflowItems?.[0]?.eventType, 'command_session_summary');
  assert.equal(assistant.workflowItems?.[0]?.status, 'completed');
  assert.equal(assistant.workflowItems?.[0]?.commandSessionId, 'cmd-1');
  assert.equal(assistant.workflowItems?.[0]?.isTool, true);
});

test('canonical thread control event projects a workflow item', () => {
  const projection = createChatRuntimeProjection();
  const events = buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'thread_control',
    eventId: 45,
    requestId: 'req-thread-control',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        action: 'switch',
        switch_session: {
          id: 'session-target',
          title: 'Target'
        }
      }
    }
  });

  assert.equal(events.length, 1);
  assert.equal(events[0].event_type, 'workflow_event');
  events.forEach((event) => applyChatRuntimeEvent(projection, event));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  const assistant = visible.find((message) => message.role === 'assistant');
  assert.ok(assistant);
  assert.equal(assistant.workflowItems?.length, 1);
  assert.equal(assistant.workflowItems?.[0]?.eventType, 'thread_control');
  assert.equal(assistant.workflowItems?.[0]?.status, 'completed');
});

test('canonical plan and question panel events project visible display state', () => {
  const projection = createChatRuntimeProjection();

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'plan_update',
    eventId: 50,
    requestId: 'req-panels',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        explanation: 'planned route',
        plan: [
          { step: 'collect input', status: 'completed' },
          { step: 'run task', status: 'in_progress' },
          { step: 'summarize', status: 'pending' }
        ]
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'question_panel',
    eventId: 51,
    requestId: 'req-panels',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        question: 'Pick a route',
        routes: [
          { label: 'Fast', description: 'Use default settings', recommended: true },
          { label: 'Careful', description: 'Use extended checks' }
        ],
        multiple: false
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  const assistant = visible.find((message) => message.role === 'assistant');
  assert.ok(assistant);
  assert.equal(assistant.display?.plan?.explanation, 'planned route');
  assert.equal(Array.isArray((assistant.display?.plan as { steps?: unknown[] } | undefined)?.steps), true);
  assert.equal((assistant.display?.plan as { steps?: Array<{ status?: string }> })?.steps?.[1]?.status, 'in_progress');
  assert.equal(assistant.display?.questionPanel?.question, 'Pick a route');
  assert.equal((assistant.display?.questionPanel as { status?: string })?.status, 'pending');
  assert.equal((assistant.display?.questionPanel as { routes?: Array<{ label?: string; recommended?: boolean }> })?.routes?.[0]?.label, 'Fast');
  assert.equal((assistant.display?.questionPanel as { routes?: Array<{ label?: string; recommended?: boolean }> })?.routes?.[0]?.recommended, true);
  assert.equal(assistant.workflowItems?.some((item) => item.eventType === 'plan_update'), true);
  assert.equal(assistant.workflowItems?.some((item) => item.eventType === 'question_panel'), true);
});

test('canonical usage and context events project assistant stats display state', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_message_created',
    event_id: 'evt-usage-1',
    event_seq: 1,
    user_turn_id: 'ut-usage-1',
    model_turn_id: 'mt-usage-1',
    message_id: 'am-usage-1'
  }));

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'token_usage',
    eventId: 2,
    requestId: 'req-usage',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        input_tokens: 90,
        output_tokens: 10,
        total_tokens: 100,
        context_occupancy_tokens: 120,
        max_context: 1000,
        decode_duration_s: 2,
        avg_model_round_speed_tps: 5,
        avg_model_round_speed_rounds: 1
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'round_usage',
    eventId: 3,
    requestId: 'req-usage',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        input_tokens: 110,
        output_tokens: 15,
        total_tokens: 125,
        request_consumed_tokens: 125,
        context_occupancy_tokens: 150,
        max_context: 1000
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  buildCanonicalChatRuntimeEvents({
    sessionId: 'session-1',
    eventType: 'quota_usage',
    eventId: 4,
    requestId: 'req-usage',
    payload: {
      data: {
        user_round: 1,
        model_round: 1,
        consumed: 125,
        daily_quota: 5000,
        used: 1250,
        remaining: 3750,
        date: '2026-04-30'
      }
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_final',
    event_id: 'evt-usage-5',
    event_seq: 5,
    user_turn_id: 'ut-usage-1',
    model_turn_id: 'mt-usage-1',
    message_id: 'am-usage-1',
    content: 'done'
  }));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  const assistant = visible.find((message) => message.role === 'assistant');
  assert.ok(assistant);
  assert.equal(assistant.status, 'final');
  assert.equal(assistant.workflowItems?.length ?? 0, 0);
  assert.deepEqual(assistant.display?.stats?.usage, { input: 90, output: 10, total: 100 });
  assert.deepEqual(assistant.display?.stats?.roundUsage, { input: 110, output: 15, total: 125 });
  assert.equal(assistant.display?.stats?.contextTokens, 150);
  assert.equal(assistant.display?.stats?.context_occupancy_tokens, 150);
  assert.equal(assistant.display?.stats?.contextTotalTokens, 1000);
  assert.equal(assistant.display?.stats?.quotaConsumed, 125);
  assert.equal((assistant.display?.stats?.quotaSnapshot as { remaining?: number })?.remaining, 3750);
  assert.equal(assistant.display?.context_occupancy_tokens, 150);
  assert.equal(assistant.display?.quotaConsumed, 125);
  assert.equal(assistant.display?.stats?.decode_duration_s, 2);
  assert.equal(assistant.display?.stats?.avg_model_round_speed_tps, 5);
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
  assert.equal(selectRuntimeLastAppliedEventId(projection, 'session-1'), 0);
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
  assert.equal(selectRuntimeLastAppliedEventId(projection, 'session-1'), 0);
  assert.equal(projection.sessions['session-1'].pendingSequentialEvents.length, 0);
});

test('runtime reducer exposes only applied numeric event ids as replay cursor', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 41,
    event_seq: 41,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    delta: 'A'
  }));
  const pending = applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 43,
    event_seq: 43,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    delta: 'C'
  }));

  assert.equal(pending.pending, true);
  assert.equal(selectRuntimeLastAppliedEventId(projection, 'session-1'), 41);

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_delta',
    event_id: 42,
    event_seq: 42,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    delta: 'B'
  }));

  assert.equal(selectRuntimeLastAppliedEventId(projection, 'session-1'), 43);
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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
    event_type: 'session_snapshot',
    source: 'snapshot',
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

test('runtime selector orders turns by event sequence instead of wall clock', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'user_message_created',
    event_id: 'evt-user-1',
    event_seq: 1,
    user_turn_id: 'ut-1',
    message_id: 'um-1',
    content: 'first',
    created_at: '2026-04-30T02:20:00.000Z'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_final',
    event_id: 'evt-assistant-1',
    event_seq: 2,
    user_turn_id: 'ut-1',
    model_turn_id: 'mt-1',
    message_id: 'am-1',
    content: 'first answer',
    created_at: '2026-04-30T02:20:01.000Z'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'user_message_created',
    event_id: 'evt-user-2',
    event_seq: 3,
    user_turn_id: 'ut-2',
    message_id: 'um-2',
    content: 'second',
    created_at: '2026-04-30T02:10:00.000Z'
  }));
  applyChatRuntimeEvent(projection, baseEvent({
    event_type: 'assistant_final',
    event_id: 'evt-assistant-2',
    event_seq: 4,
    user_turn_id: 'ut-2',
    model_turn_id: 'mt-2',
    message_id: 'am-2',
    content: 'second answer',
    created_at: '2026-04-30T02:10:01.000Z'
  }));

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['user:first', 'assistant:first answer', 'user:second', 'assistant:second answer']
  );
});

test('runtime reducer folds stop artifacts before the next optimistic turn', () => {
  const projection = createChatRuntimeProjection();

  applyChatRuntimeEvent(
    projection,
    buildCanonicalClientMessageSubmittedEvent({
      sessionId: 'session-1',
      content: 'first request',
      clientMessageId: 'local-user-1',
      createdAt: '2026-04-30T02:14:06.000Z',
      userTurnId: 'user-turn:session-1:round:1'
    })
  );
  applyChatRuntimeEvent(projection, {
    event_type: 'assistant_message_created',
    source: 'local',
    strict: false,
    session_id: 'session-1',
    event_id: 'local-assistant-1',
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:1',
    message_id: 'local-assistant:model-turn:session-1:user:1:model:1'
  });
  applyChatRuntimeEvent(projection, {
    event_type: 'turn_cancelled',
    source: 'local',
    strict: false,
    session_id: 'session-1',
    event_id: 'local-cancelled-1',
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1:model:1',
    message_id: 'local-assistant:model-turn:session-1:user:1:model:1',
    content: 'request aborted'
  });
  applyChatRuntimeEvent(projection, {
    event_type: 'turn_failed',
    source: 'ws',
    strict: false,
    session_id: 'session-1',
    event_id: 'server-error-1',
    user_turn_id: 'local-user-1',
    model_turn_id: 'model-turn:session-1:request:req-1',
    content: 'server cancelled',
    payload: {
      client_message_id: 'local-user-1'
    }
  });
  applyChatRuntimeEvent(projection, {
    event_type: 'turn_cancelled',
    source: 'ws',
    strict: false,
    session_id: 'session-1',
    event_id: 'server-terminal-1',
    user_turn_id: 'user-turn:session-1:round:1',
    model_turn_id: 'model-turn:session-1:user:1',
    payload: {
      client_message_id: 'local-user-1'
    }
  });

  applyChatRuntimeEvent(
    projection,
    buildCanonicalClientMessageSubmittedEvent({
      sessionId: 'session-1',
      content: 'continue',
      clientMessageId: 'local-user-2',
      createdAt: '2026-04-30T02:14:10.000Z',
      userTurnId: 'user-turn:session-1:round:2'
    })
  );
  applyChatRuntimeEvent(projection, {
    event_type: 'assistant_message_created',
    source: 'local',
    strict: false,
    session_id: 'session-1',
    event_id: 'local-assistant-2',
    user_turn_id: 'user-turn:session-1:round:2',
    model_turn_id: 'model-turn:session-1:user:2:model:1',
    message_id: 'local-assistant:model-turn:session-1:user:2:model:1'
  });
  applyChatRuntimeEvent(projection, {
    event_type: 'assistant_output_snapshot',
    source: 'ws',
    strict: false,
    session_id: 'session-1',
    event_id: 'server-output-2-a',
    user_turn_id: 'user-turn:session-1:round:2',
    model_turn_id: 'model-turn:session-1:user:2:model:1',
    message_id: 'assistant-message:model-turn:session-1:user:2:model:1',
    content: 'draft',
    payload: {
      client_message_id: 'local-user-2'
    }
  });
  applyChatRuntimeEvent(projection, {
    event_type: 'assistant_final',
    source: 'ws',
    strict: false,
    session_id: 'session-1',
    event_id: 'server-output-2-b',
    user_turn_id: 'user-turn:session-1:round:2',
    model_turn_id: 'model-turn:session-1:user:2:model:3',
    message_id: 'assistant-message:model-turn:session-1:user:2:model:3',
    content: 'done',
    payload: {
      client_message_id: 'local-user-2'
    }
  });

  const visible = selectVisibleMessageProjections(projection, 'session-1');
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}:${message.status}`),
    [
      'user:first request:final',
      'assistant:request aborted:cancelled',
      'user:continue:final',
      'assistant:done:final'
    ]
  );
  assert.equal(
    visible.filter((message) => message.role === 'assistant').length,
    2
  );
  assert.equal(projection.sessions['session-1'].userTurns.length, 2);
});

test('strict runtime reducer expires small event_seq gaps and asks for replay', () => {
  const projection = createChatRuntimeProjection();
  const originalNow = Date.now;
  let now = 1_000;
  Date.now = () => now;
  try {
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
    assert.equal(pending.pending, true);
    assert.equal(projection.sessions['session-1'].pendingSequentialEvents.length, 1);

    now += 801;
    const timeout = applyChatRuntimeEvent(projection, baseEvent({
      event_type: 'assistant_delta',
      event_id: 'evt-4',
      event_seq: 4,
      user_turn_id: 'ut-1',
      model_turn_id: 'mt-1',
      message_id: 'am-1',
      delta: 'D'
    }));

    const session = projection.sessions['session-1'];
    const visible = selectVisibleMessageProjections(projection, 'session-1');
    assert.equal(timeout.applied, true);
    assert.equal(timeout.reason, 'event_seq_gap_timeout');
    assert.equal(session.syncRequired, true);
    assert.equal(session.pendingSequentialEvents.length, 0);
    assert.equal(visible[0].content, 'AD');
    assert.ok(session.invariantViolations.some((violation) => violation.code === 'event_seq_gap_timeout'));
  } finally {
    Date.now = originalNow;
  }
});
