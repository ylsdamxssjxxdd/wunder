import test from 'node:test';
import assert from 'node:assert/strict';

import { createChatRuntimeProjection, applyChatRuntimeEvent } from '../../src/realtime/chat/chatRuntimeReducer';
import { buildCanonicalStreamRuntimeEvents } from '../../src/realtime/chat/chatRuntimeBridge';
import {
  buildChatRuntimeRenderableMessages,
  hasChatRuntimeRenderSession,
  isChatRuntimeProjectionRenderEnabled,
  materializeChatRuntimeMessage,
  materializeChatRuntimeMessages,
  resolveChatRuntimeProjectionRenderMode,
  resolveChatRuntimeRenderableSourceDecision,
  resolveChatRuntimeMessageRenderKey
} from '../../src/realtime/chat/chatRuntimeRenderAdapter';
import { resolveComposerContextUsageSource } from '../../src/components/chat/composerContextUsage';
import { buildAssistantMessageStatsEntries } from '../../src/utils/messageStats';
import { resolveAssistantMessageRuntimeState } from '../../src/utils/assistantMessageRuntime';
import type { ChatRuntimeEvent } from '../../src/realtime/chat/chatRuntimeTypes';
import { buildBoundedStructuralRevision } from '../../src/utils/boundedStructuralRevision';

const apply = (events: ChatRuntimeEvent[]) => {
  const projection = createChatRuntimeProjection();
  events.forEach((event) => applyChatRuntimeEvent(projection, event));
  return projection;
};

const withWindowFlags = (
  flags: Record<string, string>,
  fn: () => void,
  search = ''
) => {
  const previousWindow = (globalThis as Record<string, unknown>).window;
  const storage = new Map<string, string>(Object.entries(flags));
  (globalThis as Record<string, unknown>).window = {
    localStorage: {
      getItem: (key: string) => storage.get(key) ?? null
    },
    location: { search }
  };
  try {
    fn();
  } finally {
    if (previousWindow === undefined) {
      delete (globalThis as Record<string, unknown>).window;
    } else {
      (globalThis as Record<string, unknown>).window = previousWindow;
    }
  }
};

const t = (key: string, params?: Record<string, unknown>): string => {
  const table: Record<string, string> = {
    'chat.stats.duration': 'Duration',
    'chat.stats.speed': 'Speed',
    'chat.stats.contextTokens': 'Context',
    'chat.stats.quota': 'Quota',
    'chat.stats.toolCalls': 'Tools',
    'chat.stats.userRoundStatus': `Round ${String(params?.round ?? '')}`,
    'messenger.messageStatus.done': 'Done',
    'messenger.messageStatus.requesting': 'Requesting',
    'messenger.messageStatus.modelOutputting': 'Outputting',
    'messenger.messageStatus.queued': 'Queued',
    'messenger.messageStatus.queuedAhead': `Queued · ${String(params?.count ?? '')} ahead`
  };
  return table[key] || key;
};

test('chat runtime render adapter keeps stable keys across streaming updates', () => {
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(projection, {
    event_type: 'assistant_delta',
    source: 'test',
    strict: true,
    session_id: 'session-1',
    event_id: 'event-1',
    event_seq: 1,
    user_turn_id: 'turn-1',
    model_turn_id: 'model-turn-1',
    message_id: 'message-assistant-1',
    delta: 'hello'
  });
  const first = buildChatRuntimeRenderableMessages({
    projection,
    sessionId: 'session-1'
  });

  applyChatRuntimeEvent(projection, {
    event_type: 'assistant_delta',
    source: 'test',
    strict: true,
    session_id: 'session-1',
    event_id: 'event-2',
    event_seq: 2,
    user_turn_id: 'turn-1',
    model_turn_id: 'model-turn-1',
    message_id: 'message-assistant-1',
    delta: ' world'
  });
  const second = buildChatRuntimeRenderableMessages({
    projection,
    sessionId: 'session-1'
  });

  assert.equal(first.length, 1);
  assert.equal(second.length, 1);
  assert.equal(first[0].key, 'runtime:assistant:message-assistant-1');
  assert.equal(second[0].key, first[0].key);
  assert.equal(second[0].message.content, 'hello world');
  assert.equal(second[0].sourceIndex, 0);
});

test('chat runtime render adapter reuses materialized objects across steady content changes', () => {
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(projection, {
    event_type: 'assistant_delta',
    source: 'test',
    strict: true,
    session_id: 'session-1',
    event_id: 'event-cache-1',
    event_seq: 1,
    user_turn_id: 'turn-cache-1',
    model_turn_id: 'model-turn-cache-1',
    message_id: 'message-cache-1',
    delta: 'stable'
  });

  const first = materializeChatRuntimeMessages(projection, 'session-1');
  const second = materializeChatRuntimeMessages(projection, 'session-1');

  assert.equal(first[0], second[0]);
  second[0].content = 'external mutation';

  const recovered = materializeChatRuntimeMessages(projection, 'session-1');
  assert.equal(recovered[0], second[0]);
  assert.equal(recovered[0].content, 'stable');

  applyChatRuntimeEvent(projection, {
    event_type: 'assistant_delta',
    source: 'test',
    strict: true,
    session_id: 'session-1',
    event_id: 'event-cache-2',
    event_seq: 2,
    user_turn_id: 'turn-cache-1',
    model_turn_id: 'model-turn-cache-1',
    message_id: 'message-cache-1',
    delta: ' update'
  });
  const updated = materializeChatRuntimeMessages(projection, 'session-1');

  assert.equal(updated[0], recovered[0]);
  assert.equal(updated[0].content, 'stable update');
});

test('chat runtime render adapter preserves untouched workflow row identity', () => {
  const sessionId = 'session-workflow-row-cache';
  const projection = apply([
    {
      event_type: 'session_snapshot',
      source: 'snapshot',
      strict: false,
      session_id: sessionId,
      messages: [{
        message_id: 'assistant-workflow-row-cache',
        user_turn_id: 'turn-workflow-row-cache',
        model_turn_id: 'model-workflow-row-cache',
        turn_index: 1,
        role: 'assistant',
        content: '',
        status: 'streaming',
        workflowItems: [
          {
            id: 'tool-row-1',
            eventType: 'tool_result',
            status: 'completed',
            toolCallId: 'tool-row-1',
            updatedSeq: 1,
            detail: 'first result'
          },
          {
            id: 'tool-row-2',
            eventType: 'tool_output_delta',
            status: 'loading',
            toolCallId: 'tool-row-2',
            updatedSeq: 2,
            detail: 'initial output'
          }
        ]
      }],
      loading: false,
      running: true
    }
  ]);

  const first = materializeChatRuntimeMessages(projection, sessionId)[0];
  const firstRows = first.workflowItems as Array<Record<string, unknown>>;
  const firstRow = firstRows[0];
  const secondRow = firstRows[1];
  const projected = projection.sessions[sessionId]?.messageById['assistant-workflow-row-cache'];
  assert.ok(projected);
  (projected?.workflowItems?.[1] as Record<string, unknown>).detail = 'updated output';
  (projected?.workflowItems?.[1] as Record<string, unknown>).updatedSeq = 3;
  projected!.structureVersion = Number(projected!.structureVersion || 0) + 1;

  const second = materializeChatRuntimeMessages(projection, sessionId)[0];
  const secondRows = second.workflowItems as Array<Record<string, unknown>>;
  assert.equal(second, first);
  assert.equal(secondRows, firstRows);
  assert.equal(secondRows[0], firstRow);
  assert.equal(secondRows[1], secondRow);
  assert.equal(secondRows[1]?.detail, 'updated output');
});

test('chat runtime render adapter retains the latest user-turn workflow after completion', () => {
  const sessionId = 'session-active-workflow-window';
  const projection = apply([{
    event_type: 'session_snapshot',
    source: 'snapshot',
    strict: false,
    session_id: sessionId,
    messages: [
      {
        message_id: 'user-history',
        user_turn_id: 'turn-history',
        turn_index: 1,
        role: 'user',
        content: 'earlier request'
      },
      {
        message_id: 'assistant-history',
        user_turn_id: 'turn-history',
        model_turn_id: 'model-history',
        turn_index: 1,
        role: 'assistant',
        content: 'previous response',
        workflowItems: [{
          id: 'tool-history',
          eventType: 'tool_result',
          status: 'completed',
          detail: 'previous detail'
        }]
      },
      {
        message_id: 'assistant-active',
        user_turn_id: 'turn-active',
        model_turn_id: 'model-active',
        turn_index: 2,
        role: 'assistant',
        content: '',
        workflowItems: [{
          id: 'tool-active',
          eventType: 'tool_call',
          status: 'loading',
          detail: 'active detail'
        }]
      }
    ],
    running: true
  }, {
    event_type: 'session_runtime',
    source: 'snapshot',
    strict: false,
    session_id: sessionId,
    runtime_status: 'running'
  }]);

  const materialized = buildChatRuntimeRenderableMessages({ projection, sessionId })
    .map((item) => item.message);
  const history = materialized.find((message) => message.message_id === 'assistant-history');
  const active = materialized.find((message) => message.message_id === 'assistant-active');

  assert.deepEqual(history?.workflowItems, []);
  assert.equal(history?.workflowStreaming, false);
  assert.equal((active?.workflowItems as Array<Record<string, unknown>>).length, 1);
  assert.equal(active?.workflowStreaming, true);

  applyChatRuntimeEvent(projection, {
    event_type: 'turn_completed',
    source: 'test',
    strict: true,
    session_id: sessionId,
    event_id: 'active-complete',
    event_seq: 10,
    user_turn_id: 'turn-active',
    model_turn_id: 'model-active'
  });

  const completed = buildChatRuntimeRenderableMessages({ projection, sessionId })
    .map((item) => item.message);
  const completedHistory = completed.find((message) => message.message_id === 'assistant-history');
  const completedActive = completed.find((message) => message.message_id === 'assistant-active');

  assert.deepEqual(completedHistory?.workflowItems, []);
  assert.equal((completedActive?.workflowItems as Array<Record<string, unknown>>).length, 1);
  assert.equal(completedActive?.workflowStreaming, false);
  assert.equal(completedActive?.stream_incomplete, false);

  applyChatRuntimeEvent(projection, {
    event_type: 'user_message_created',
    source: 'test',
    strict: true,
    session_id: sessionId,
    event_id: 'next-user-message',
    event_seq: 11,
    user_turn_id: 'turn-next',
    message_id: 'user-next',
    content: 'next question'
  });
  applyChatRuntimeEvent(projection, {
    event_type: 'tool_call_started',
    source: 'test',
    strict: true,
    session_id: sessionId,
    event_id: 'next-tool-call',
    event_seq: 12,
    user_turn_id: 'turn-next',
    model_turn_id: 'model-next',
    message_id: 'assistant-next',
    tool_call_id: 'tool-next',
    tool_name: 'lookup'
  });

  const next = buildChatRuntimeRenderableMessages({ projection, sessionId })
    .map((item) => item.message);
  const previous = next.find((message) => message.message_id === 'assistant-active');
  const nextActive = next.find((message) => message.message_id === 'assistant-next');

  assert.deepEqual(previous?.workflowItems, []);
  assert.equal((nextActive?.workflowItems as Array<Record<string, unknown>>).length, 1);
});

test('chat runtime render adapter retains every model loop in the latest user turn', () => {
  const sessionId = 'session-user-turn-workflow-window';
  const projection = apply([{
    event_type: 'session_snapshot',
    source: 'snapshot',
    strict: false,
    session_id: sessionId,
    messages: [
      {
        message_id: 'user-history',
        user_turn_id: 'turn-history',
        turn_index: 1,
        role: 'user',
        content: 'earlier request'
      },
      {
        message_id: 'assistant-history',
        user_turn_id: 'turn-history',
        model_turn_id: 'model-history',
        turn_index: 1,
        role: 'assistant',
        content: 'previous response',
        workflowItems: [{ id: 'history-tool', eventType: 'tool_result', status: 'completed' }]
      },
      {
        message_id: 'user-current',
        user_turn_id: 'turn-current',
        turn_index: 2,
        role: 'user',
        content: 'current request'
      },
      {
        message_id: 'assistant-image',
        user_turn_id: 'turn-current',
        model_turn_id: 'model-image',
        turn_index: 2,
        role: 'assistant',
        content: 'inspection complete',
        workflowItems: [{
          id: 'image-tool',
          eventType: 'tool_result',
          status: 'completed',
          toolName: 'read_image',
          toolCallId: 'image-call'
        }]
      },
      {
        message_id: 'assistant-final',
        user_turn_id: 'turn-current',
        model_turn_id: 'model-final',
        turn_index: 2,
        role: 'assistant',
        content: 'current response',
        workflowItems: [{ id: 'final-tool', eventType: 'tool_result', status: 'completed' }]
      }
    ],
    running: false
  }]);

  const materialized = buildChatRuntimeRenderableMessages({ projection, sessionId })
    .map((item) => item.message);
  const history = materialized.find((message) => message.message_id === 'assistant-history');
  const imageLoop = materialized.find((message) => message.message_id === 'assistant-image');
  const finalLoop = materialized.find((message) => message.message_id === 'assistant-final');

  assert.deepEqual(history?.workflowItems, []);
  assert.equal((imageLoop?.workflowItems as Array<Record<string, unknown>>).length, 1);
  assert.equal((finalLoop?.workflowItems as Array<Record<string, unknown>>).length, 1);
  assert.equal(
    (imageLoop?.workflowItems as Array<Record<string, unknown>>)[0]?.toolName,
    'read_image'
  );
});

test('chat runtime render adapter restores an active workflow placeholder after snapshot hydration', () => {
  const sessionId = 'session-workflow-refresh-placeholder';
  const projection = apply([{
    event_type: 'session_snapshot',
    source: 'snapshot',
    strict: false,
    session_id: sessionId,
    messages: [
      {
        message_id: 'assistant-history',
        user_turn_id: 'turn-history',
        model_turn_id: 'model-history',
        turn_index: 1,
        role: 'assistant',
        content: 'previous response',
        workflowItems: [{ id: 'tool-history', eventType: 'tool_result', status: 'completed' }]
      },
      {
        message_id: 'assistant-active',
        user_turn_id: 'turn-active',
        model_turn_id: 'model-active',
        turn_index: 2,
        role: 'assistant',
        content: 'partial response',
        status: 'final'
      }
    ],
    running: true
  }, {
    event_type: 'session_runtime',
    source: 'snapshot',
    strict: false,
    session_id: sessionId,
    runtime_status: 'running'
  }]);

  const materialized = buildChatRuntimeRenderableMessages({ projection, sessionId })
    .map((item) => item.message);
  const history = materialized.find((message) => message.message_id === 'assistant-history');
  const active = materialized.find((message) => message.message_id === 'assistant-active');

  assert.deepEqual(history?.workflowItems, []);
  assert.equal(history?.workflowStreaming, false);
  assert.deepEqual(active?.workflowItems, []);
  assert.equal(active?.workflowStreaming, true);
  assert.deepEqual(active?.workflowPendingPlaceholder, {
    kind: 'tool',
    toolName: '',
    toolDisplayName: '',
    toolRuntimeName: '',
    toolFunctionName: '',
    eventType: 'runtime_pending'
  });
});

test('chat runtime render adapter keeps the latest workflow while final text settles', () => {
  const sessionId = 'session-workflow-final-text';
  const projection = createChatRuntimeProjection();
  const event = (event_type: string, event_id: string, event_seq: number, extra: Record<string, unknown> = {}) => ({
    event_type,
    source: 'test',
    strict: true,
    session_id: sessionId,
    event_id,
    event_seq,
    user_turn_id: 'turn-1',
    model_turn_id: 'model-1',
    message_id: 'assistant-1',
    ...extra
  });

  applyChatRuntimeEvent(projection, event('tool_call_started', 'tool-start', 1, {
    payload: {
      tool_call_id: 'call-1',
      tool_name: 'lookup'
    }
  }));
  const duringTool = buildChatRuntimeRenderableMessages({ projection, sessionId })
    .map((item) => item.message)
    .find((message) => message.message_id === 'assistant-1');
  assert.equal((duringTool?.workflowItems as Array<Record<string, unknown>>).length, 1);
  assert.equal(duringTool?.workflowStreaming, true);

  applyChatRuntimeEvent(projection, event('tool_call_completed', 'tool-complete', 2, {
    payload: {
      tool_call_id: 'call-1',
      tool_name: 'lookup'
    }
  }));
  applyChatRuntimeEvent(projection, event('assistant_delta', 'answer-delta', 3, {
    delta: 'partial answer'
  }));
  const duringAnswer = buildChatRuntimeRenderableMessages({ projection, sessionId })
    .map((item) => item.message)
    .find((message) => message.message_id === 'assistant-1');
  assert.equal((duringAnswer?.workflowItems as Array<Record<string, unknown>>).length, 1);
  assert.equal(duringAnswer?.content, 'partial answer');

  applyChatRuntimeEvent(projection, event('assistant_final', 'answer-final', 4, {
    content: 'final answer'
  }));
  const finalMessage = buildChatRuntimeRenderableMessages({ projection, sessionId })
    .map((item) => item.message)
    .find((message) => message.message_id === 'assistant-1');
  const finalItems = finalMessage?.workflowItems as Array<Record<string, unknown>>;

  assert.equal(finalMessage?.content, 'final answer');
  assert.equal(finalItems.length, 1);
  assert.equal(finalItems[0]?.status, 'completed');
  assert.equal(finalMessage?.workflowStreaming, false);
  assert.equal(finalMessage?.stream_incomplete, false);
});

test('bounded structural revisions avoid scanning large detail strings while tracking runtime sequence changes', () => {
  const detail = 'x'.repeat(500_000);
  const first = buildBoundedStructuralRevision([{ id: 'tool-1', updatedSeq: 1, detail }]);
  const second = buildBoundedStructuralRevision([{ id: 'tool-1', updatedSeq: 2, detail }]);

  assert.notEqual(first, second);
});

test('chat runtime render adapter orders user messages before assistant turns', () => {
  const projection = apply([
    {
      event_type: 'assistant_message_created',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-1',
      event_seq: 1,
      user_turn_id: 'turn-1',
      model_turn_id: 'model-turn-1',
      message_id: 'message-assistant-1'
    },
    {
      event_type: 'assistant_final',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-2',
      event_seq: 2,
      user_turn_id: 'turn-1',
      model_turn_id: 'model-turn-1',
      message_id: 'message-assistant-1',
      content: 'answer'
    },
    {
      event_type: 'user_message_created',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-3',
      event_seq: 3,
      user_turn_id: 'turn-1',
      message_id: 'message-user-1',
      content: 'question'
    }
  ]);

  const renderable = buildChatRuntimeRenderableMessages({
    projection,
    sessionId: 'session-1'
  });

  assert.deepEqual(
    renderable.map((item) => `${item.message.role}:${item.message.content}`),
    ['user:question', 'assistant:answer']
  );
});

test('chat runtime render adapter materializes projection-owned messages with legacy fields', () => {
  const user = {
    message_id: 'message-user-1',
    role: 'user',
    content: 'hello',
    created_at: '2026-04-30T02:14:06.000Z',
    attachments: [{ name: 'file' }]
  };
  const assistant = {
    message_id: 'message-assistant-1',
    role: 'assistant',
    content: 'hi',
    reasoning: '',
    created_at: '2026-04-30T02:14:07.000Z',
    stream_incomplete: false,
    workflowStreaming: false,
    reasoningStreaming: false,
    feedback: { vote: 'up' }
  };
  const projection = apply([
    {
      event_type: 'session_snapshot',
      source: 'snapshot',
      strict: false,
      session_id: 'session-1',
      messages: [user, assistant],
      loading: false,
      running: false
    }
  ]);

  const materialized = materializeChatRuntimeMessages(projection, 'session-1');

  assert.notEqual(materialized[0], user);
  assert.notEqual(materialized[1], assistant);
  assert.equal(materialized[0].__runtime_projected, true);
  assert.equal(materialized[1].__runtime_projected, true);
  assert.equal('__runtime_raw_message' in materialized[0], false);
  assert.equal('__runtime_raw_message' in materialized[1], false);
  assert.deepEqual(materialized[0].attachments, [{ name: 'file' }]);
  assert.deepEqual(materialized[1].feedback, { vote: 'up' });
  (materialized[0].attachments as Array<Record<string, unknown>>)[0].name = 'changed';
  assert.deepEqual(user.attachments, [{ name: 'file' }]);
});

test('chat runtime render adapter materializes queued stream events as queued assistant status', () => {
  const projection = createChatRuntimeProjection();
  const sessionId = 'session-queued-render';
  const userTurnId = `user-turn:${sessionId}:round:1`;
  const modelTurnId = `model-turn:${sessionId}:user:1:model:1`;
  const assistantMessageId = `local-assistant:${modelTurnId}`;

  applyChatRuntimeEvent(projection, {
    event_type: 'client_message_submitted',
    source: 'local',
    strict: false,
    session_id: sessionId,
    event_id: 'local-submit-queued',
    user_turn_id: userTurnId,
    message_id: `local-user:${sessionId}:1`,
    content: 'hello'
  });
  applyChatRuntimeEvent(projection, {
    event_type: 'assistant_message_created',
    source: 'local',
    strict: false,
    session_id: sessionId,
    event_id: 'local-assistant-queued',
    user_turn_id: userTurnId,
    model_turn_id: modelTurnId,
    message_id: assistantMessageId
  });

  buildCanonicalStreamRuntimeEvents({
    sessionId,
    eventType: 'queue_enter',
    eventId: 10,
    requestId: 'request-queued-render',
    phase: 'send',
    source: 'ws',
    userTurnId,
    modelTurnId,
    assistantMessageId,
    payload: {
      event_id: 10,
      queue_id: 'queue-render-1',
      wait_ahead: 2
    }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));

  const materialized = materializeChatRuntimeMessages(projection, sessionId);
  const assistant = materialized.find((message) => message.role === 'assistant');
  assert.equal(assistant?.state, 'queued');
  assert.equal(assistant?.runtime_status, 'queued');
  assert.equal(assistant?.workflowStreaming, true);

  const entries = buildAssistantMessageStatsEntries(
    assistant as Record<string, any>,
    t,
    materialized as Array<Record<string, any>>,
    Date.UTC(2026, 6, 9, 12, 0, 0),
    {
      activeSessionBusy: true,
      latestVisibleAssistant: true
    }
  );
  assert.equal(entries[0]?.value, 'Queued · 2 ahead');
});

test('chat runtime render adapter coalesces queue-start transient assistant duplicates', () => {
  const projection = createChatRuntimeProjection();
  const sessionId = 'session-queue-render-duplicates';
  const canonicalUserTurnId = `user-turn:${sessionId}:round:1`;
  const weakUserTurnId = 'queue-turn:task-render:user';
  const weakModelTurnId = `model-turn:${sessionId}:user:1`;
  const strongModelTurnId = `model-turn:${sessionId}:user:1:model:1`;
  const weakMessageId = `assistant-message:${weakModelTurnId}`;
  const strongMessageId = `assistant-message:${strongModelTurnId}`;

  projection.sessions[sessionId] = {
    sessionId,
    agentId: '',
    appliedSeq: 0,
    lastAppliedEventId: 0,
    snapshotSeq: 0,
    localSeq: 0,
    syncRequired: false,
    connectionState: 'connected',
    runtimeStatus: 'running',
    busyReason: 'streaming',
    eventIdIndex: {},
    userTurns: [canonicalUserTurnId, weakUserTurnId],
    modelTurns: ['queue:task-render:assistant', weakModelTurnId, strongModelTurnId],
    messages: [
      `local-user:${sessionId}:1`,
      'queue:task-render:assistant-message',
      weakMessageId,
      strongMessageId
    ],
    messageById: {
      [`local-user:${sessionId}:1`]: {
        id: `local-user:${sessionId}:1`,
        role: 'user',
        content: 'question',
        reasoning: '',
        status: 'final',
        createdAt: '2026-04-30T02:14:06.000Z',
        createdSeq: 1,
        updatedSeq: 1,
        userTurnId: canonicalUserTurnId,
        modelTurnId: '',
        final: true,
        failed: false,
        cancelled: false
      },
      'queue:task-render:assistant-message': {
        id: 'queue:task-render:assistant-message',
        role: 'assistant',
        content: '',
        reasoning: '',
        status: 'waiting_first_output',
        createdAt: '2026-04-30T02:14:07.000Z',
        createdSeq: 2,
        updatedSeq: 2,
        userTurnId: weakUserTurnId,
        modelTurnId: 'queue:task-render:assistant',
        final: false,
        failed: false,
        cancelled: false,
        workflowItems: [{
          id: 'queue:status',
          eventType: 'queue_start',
          status: 'running'
        }],
        subagents: []
      },
      [weakMessageId]: {
        id: weakMessageId,
        role: 'assistant',
        content: 'first text',
        reasoning: '',
        status: 'streaming',
        createdAt: '2026-04-30T02:14:08.000Z',
        createdSeq: 3,
        updatedSeq: 3,
        userTurnId: weakUserTurnId,
        modelTurnId: weakModelTurnId,
        final: false,
        failed: false,
        cancelled: false,
        workflowItems: [],
        subagents: []
      },
      [strongMessageId]: {
        id: strongMessageId,
        role: 'assistant',
        content: 'first text continued',
        reasoning: '',
        status: 'streaming',
        createdAt: '2026-04-30T02:14:09.000Z',
        createdSeq: 4,
        updatedSeq: 4,
        userTurnId: canonicalUserTurnId,
        modelTurnId: strongModelTurnId,
        final: false,
        failed: false,
        cancelled: false,
        workflowItems: [],
        subagents: []
      }
    },
    userTurnById: {
      [canonicalUserTurnId]: {
        id: canonicalUserTurnId,
        createdSeq: 1,
        messageIds: [`local-user:${sessionId}:1`],
        modelTurnIds: [strongModelTurnId],
        status: 'model_running'
      },
      [weakUserTurnId]: {
        id: weakUserTurnId,
        createdSeq: 2,
        messageIds: [],
        modelTurnIds: ['queue:task-render:assistant', weakModelTurnId],
        status: 'model_running'
      }
    },
    modelTurnById: {
      'queue:task-render:assistant': {
        id: 'queue:task-render:assistant',
        userTurnId: weakUserTurnId,
        createdSeq: 2,
        messageIds: ['queue:task-render:assistant-message'],
        finalMessageId: '',
        status: 'waiting_first_output'
      },
      [weakModelTurnId]: {
        id: weakModelTurnId,
        userTurnId: weakUserTurnId,
        createdSeq: 3,
        messageIds: [weakMessageId],
        finalMessageId: '',
        status: 'streaming'
      },
      [strongModelTurnId]: {
        id: strongModelTurnId,
        userTurnId: canonicalUserTurnId,
        createdSeq: 4,
        messageIds: [strongMessageId],
        finalMessageId: '',
        status: 'streaming'
      }
    },
    invariantViolations: [],
    quarantinedEvents: [],
    pendingSequentialEvents: []
  };

  const materialized = materializeChatRuntimeMessages(projection, sessionId);
  const assistants = materialized.filter((message) => message.role === 'assistant');

  assert.equal(assistants.length, 1);
  assert.equal(assistants[0]?.message_id, strongMessageId);
  assert.equal(assistants[0]?.content, 'first text continued');
  assert.equal((assistants[0]?.workflowItems as unknown[])?.length, 1);
});

test('chat runtime render adapter coalesces hydrated answer with replayed workflow projection', () => {
  const projection = createChatRuntimeProjection();
  const sessionId = 'session-history-workflow-duplicate';
  const userTurnId = `user-turn:${sessionId}:round:1`;
  const userMessageId = 'history:user:1';
  const historyModelTurnId = 'legacy-model-turn:history-answer-1';
  const replayModelTurnId = `model-turn:${sessionId}:user:1:model:1`;
  const historyAssistantId = 'history:assistant:1';
  const replayAssistantId = `assistant-message:${replayModelTurnId}`;

  projection.sessions[sessionId] = {
    sessionId,
    agentId: '',
    appliedSeq: 0,
    lastAppliedEventId: 0,
    snapshotSeq: 0,
    localSeq: 0,
    syncRequired: false,
    connectionState: 'connected',
    runtimeStatus: 'idle',
    busyReason: null,
    eventIdIndex: {},
    userTurns: [userTurnId],
    modelTurns: [historyModelTurnId, replayModelTurnId],
    messages: [userMessageId, historyAssistantId, replayAssistantId],
    messageById: {
      [userMessageId]: {
        id: userMessageId,
        role: 'user',
        content: 'question',
        reasoning: '',
        status: 'final',
        createdAt: '2026-04-30T02:14:06.000Z',
        createdSeq: 1,
        updatedSeq: 1,
        userTurnId,
        modelTurnId: '',
        final: true,
        failed: false,
        cancelled: false
      },
      [historyAssistantId]: {
        id: historyAssistantId,
        role: 'assistant',
        content: 'answer',
        reasoning: '',
        status: 'final',
        createdAt: '2026-04-30T02:14:07.000Z',
        createdSeq: 2,
        updatedSeq: 2,
        userTurnId,
        modelTurnId: historyModelTurnId,
        final: true,
        failed: false,
        cancelled: false,
        workflowItems: [],
        subagents: [],
        display: {
          stats: {
            duration: 6.87,
            speed: 43.8,
            contextTokens: 11187
          },
          user_round: 1
        }
      },
      [replayAssistantId]: {
        id: replayAssistantId,
        role: 'assistant',
        content: '',
        reasoning: '',
        status: 'final',
        createdAt: '2026-04-30T02:14:08.000Z',
        createdSeq: 3,
        updatedSeq: 3,
        userTurnId,
        modelTurnId: replayModelTurnId,
        final: true,
        failed: false,
        cancelled: false,
        workflowItems: [{
          id: 'tool:read',
          eventType: 'tool_result',
          status: 'completed',
          toolCallId: 'tool-read-1'
        }],
        subagents: [],
        display: {
          stats: {
            toolCalls: 4,
            contextTotalTokens: 100610
          }
        }
      }
    },
    userTurnById: {
      [userTurnId]: {
        id: userTurnId,
        createdSeq: 1,
        messageIds: [userMessageId],
        modelTurnIds: [historyModelTurnId, replayModelTurnId],
        status: 'completed'
      }
    },
    modelTurnById: {
      [historyModelTurnId]: {
        id: historyModelTurnId,
        userTurnId,
        createdSeq: 2,
        messageIds: [historyAssistantId],
        finalMessageId: historyAssistantId,
        status: 'completed'
      },
      [replayModelTurnId]: {
        id: replayModelTurnId,
        userTurnId,
        createdSeq: 3,
        messageIds: [replayAssistantId],
        finalMessageId: replayAssistantId,
        status: 'completed'
      }
    },
    invariantViolations: [],
    quarantinedEvents: [],
    pendingSequentialEvents: []
  };

  const materialized = materializeChatRuntimeMessages(projection, sessionId);
  const assistants = materialized.filter((message) => message.role === 'assistant');
  const assistant = assistants[0] as Record<string, any>;

  assert.equal(assistants.length, 1);
  assert.equal(assistant?.message_id, historyAssistantId);
  assert.equal(assistant?.content, 'answer');
  assert.equal(assistant?.workflowItems?.length, 1);
  assert.equal(assistant?.workflowItems?.[0]?.toolCallId, 'tool-read-1');
  assert.equal(assistant?.stats?.duration, 6.87);
  assert.equal(assistant?.stats?.toolCalls, 4);
  assert.equal(assistant?.stats?.contextTotalTokens, 100610);
});

test('chat runtime render adapter merges semantic workflow records without stable ids', () => {
  const projection = createChatRuntimeProjection();
  const sessionId = 'session-semantic-workflow';
  const userTurnId = `user-turn:${sessionId}:round:1`;
  const userMessageId = 'history:user:semantic';
  const answerModelTurnId = 'legacy-model-turn:semantic-answer';
  const workflowModelTurnId = `model-turn:${sessionId}:user:1:model:1`;
  const answerMessageId = 'history:assistant:semantic';
  const workflowMessageId = `assistant-message:${workflowModelTurnId}`;

  projection.sessions[sessionId] = {
    sessionId,
    agentId: '',
    appliedSeq: 0,
    lastAppliedEventId: 0,
    snapshotSeq: 0,
    localSeq: 0,
    syncRequired: false,
    connectionState: 'connected',
    runtimeStatus: 'idle',
    busyReason: null,
    eventIdIndex: {},
    userTurns: [userTurnId],
    modelTurns: [answerModelTurnId, workflowModelTurnId],
    messages: [userMessageId, answerMessageId, workflowMessageId],
    messageById: {
      [userMessageId]: {
        id: userMessageId,
        role: 'user',
        content: 'question',
        reasoning: '',
        status: 'final',
        createdAt: '2026-04-30T02:14:06.000Z',
        createdSeq: 1,
        updatedSeq: 1,
        userTurnId,
        modelTurnId: '',
        final: true,
        failed: false,
        cancelled: false
      },
      [answerMessageId]: {
        id: answerMessageId,
        role: 'assistant',
        content: 'answer',
        reasoning: '',
        status: 'final',
        createdAt: '2026-04-30T02:14:07.000Z',
        createdSeq: 2,
        updatedSeq: 2,
        userTurnId,
        modelTurnId: answerModelTurnId,
        final: true,
        failed: false,
        cancelled: false,
        workflowItems: [{
          eventType: 'tool_call',
          status: 'loading',
          toolName: 'lookup',
          title: 'lookup',
          modelTurnId: workflowModelTurnId,
          updatedSeq: 2
        }],
        subagents: [],
        display: { user_round: 1 }
      },
      [workflowMessageId]: {
        id: workflowMessageId,
        role: 'assistant',
        content: '',
        reasoning: '',
        status: 'final',
        createdAt: '2026-04-30T02:14:08.000Z',
        createdSeq: 3,
        updatedSeq: 3,
        userTurnId,
        modelTurnId: workflowModelTurnId,
        final: true,
        failed: false,
        cancelled: false,
        workflowItems: [{
          eventType: 'tool_result',
          status: 'completed',
          toolName: 'lookup',
          title: 'lookup',
          modelTurnId: workflowModelTurnId,
          updatedSeq: 3
        }],
        subagents: [],
        display: { user_round: 1 }
      }
    },
    userTurnById: {
      [userTurnId]: {
        id: userTurnId,
        createdSeq: 1,
        messageIds: [userMessageId],
        modelTurnIds: [answerModelTurnId, workflowModelTurnId],
        status: 'completed'
      }
    },
    modelTurnById: {
      [answerModelTurnId]: {
        id: answerModelTurnId,
        userTurnId,
        createdSeq: 2,
        messageIds: [answerMessageId],
        finalMessageId: answerMessageId,
        status: 'completed'
      },
      [workflowModelTurnId]: {
        id: workflowModelTurnId,
        userTurnId,
        createdSeq: 3,
        messageIds: [workflowMessageId],
        finalMessageId: workflowMessageId,
        status: 'completed'
      }
    },
    invariantViolations: [],
    quarantinedEvents: [],
    pendingSequentialEvents: []
  };

  const materialized = materializeChatRuntimeMessages(projection, sessionId);
  const assistants = materialized.filter((message) => message.role === 'assistant') as Array<Record<string, any>>;

  assert.equal(assistants.length, 1);
  assert.equal(assistants[0]?.workflowItems?.length, 1);
  assert.equal(assistants[0]?.workflowItems?.[0]?.eventType, 'tool_result');
  assert.equal(assistants[0]?.workflowItems?.[0]?.status, 'completed');
  assert.equal(assistants[0]?.workflowStreaming, false);
});

test('chat runtime render adapter never materializes synthetic greeting from projection', () => {
  const greeting = {
    role: 'assistant',
    content: 'synthetic greeting',
    isGreeting: true,
    created_at: '2026-04-30T02:14:01.000Z'
  };
  const user = {
    message_id: 'message-user-1',
    role: 'user',
    content: 'hello',
    created_at: '2026-04-30T02:14:06.000Z'
  };
  const projection = apply([
    {
      event_type: 'session_snapshot',
      source: 'snapshot',
      strict: false,
      session_id: 'session-1',
      messages: [greeting, user],
      loading: false,
      running: false
    }
  ]);

  const materialized = materializeChatRuntimeMessages(projection, 'session-1');
  const renderable = buildChatRuntimeRenderableMessages({
    projection,
    sessionId: 'session-1'
  });

  assert.deepEqual(
    materialized.map((message) => `${message.role}:${message.content}`),
    ['user:hello']
  );
  assert.deepEqual(
    renderable.map((item) => `${item.message.role}:${item.message.content}`),
    ['user:hello']
  );
});

test('chat runtime render adapter patches stale raw streaming flags without mutating legacy raw', () => {
  const assistant = {
    message_id: 'message-assistant-1',
    role: 'assistant',
    content: 'old',
    reasoning: '',
    created_at: '2026-04-30T02:14:07.000Z',
    stream_incomplete: false,
    workflowStreaming: false,
    reasoningStreaming: false
  };
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(projection, {
    event_type: 'session_snapshot',
    source: 'snapshot',
    strict: false,
    session_id: 'session-1',
    messages: [assistant],
    loading: false,
    running: false
  });
  applyChatRuntimeEvent(projection, {
    event_type: 'assistant_delta',
    source: 'test',
    strict: true,
    session_id: 'session-1',
    event_id: 'event-10',
    event_seq: 10,
    user_turn_id: 'legacy-user-turn:orphan:0',
    model_turn_id: 'legacy-model-turn:orphan:0',
    message_id: 'message-assistant-1',
    delta: ' live'
  });

  const materialized = materializeChatRuntimeMessages(projection, 'session-1');

  assert.notEqual(materialized[0], assistant);
  assert.equal(materialized[0].content, 'old live');
  assert.equal(materialized[0].stream_incomplete, true);
  assert.equal(assistant.stream_incomplete, false);
});

test('chat runtime render adapter does not reuse raw messages after projected workflow changes', () => {
  const assistant = {
    message_id: 'message-assistant-1',
    role: 'assistant',
    content: '',
    reasoning: '',
    created_at: '2026-04-30T02:14:07.000Z',
    stream_incomplete: true,
    workflowStreaming: true,
    reasoningStreaming: false
  };
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(projection, {
    event_type: 'session_snapshot',
    source: 'snapshot',
    strict: false,
    session_id: 'session-1',
    messages: [assistant],
    loading: true,
    running: true
  });
  applyChatRuntimeEvent(projection, {
    event_type: 'tool_call_started',
    source: 'test',
    strict: true,
    session_id: 'session-1',
    event_id: 'event-10',
    event_seq: 10,
    user_turn_id: 'legacy-user-turn:orphan:0',
    model_turn_id: 'legacy-model-turn:message-assistant-1',
    message_id: 'message-assistant-1',
    payload: {
      data: {
        tool_call_id: 'call-1',
        tool: 'lookup'
      }
    }
  });

  const materialized = materializeChatRuntimeMessages(projection, 'session-1');
  const workflowItems = materialized[0].workflowItems as Array<Record<string, unknown>>;

  assert.notEqual(materialized[0], assistant);
  assert.equal(workflowItems.length, 1);
  assert.equal(workflowItems[0].status, 'loading');
  assert.equal(workflowItems[0].toolCallId, 'call-1');
  assert.equal(Array.isArray(assistant.workflowItems), false);
});

test('chat runtime render adapter creates projection-only assistant placeholders', () => {
  const projection = apply([
    {
      event_type: 'assistant_message_created',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-1',
      event_seq: 1,
      user_turn_id: 'turn-1',
      model_turn_id: 'model-turn-1',
      message_id: 'message-assistant-1'
    }
  ]);

  const materialized = materializeChatRuntimeMessages(projection, 'session-1');

  assert.equal(materialized.length, 1);
  assert.equal(materialized[0].__runtime_projected, true);
  assert.equal(materialized[0].role, 'assistant');
  assert.equal(materialized[0].state, 'running');
  assert.equal(materialized[0].stream_incomplete, true);
  assert.equal(materialized[0].workflowStreaming, true);
  assert.deepEqual(materialized[0].workflowItems, []);
  assert.equal(resolveChatRuntimeMessageRenderKey(materialized[0]), 'runtime:assistant:message-assistant-1');
});

test('chat runtime render adapter preserves projected workflow items', () => {
  const projection = apply([
    {
      event_type: 'tool_call_started',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-1',
      event_seq: 1,
      user_turn_id: 'turn-1',
      model_turn_id: 'model-turn-1',
      message_id: 'message-assistant-1',
      payload: {
        source_event_type: 'tool_call',
        data: {
          tool_call_id: 'call-1',
          tool: 'lookup',
          query: 'status'
        }
      }
    },
    {
      event_type: 'tool_call_completed',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-2',
      event_seq: 2,
      user_turn_id: 'turn-1',
      model_turn_id: 'model-turn-1',
      message_id: 'message-assistant-1',
      payload: {
        source_event_type: 'tool_result',
        data: {
          tool_call_id: 'call-1',
          tool: 'lookup',
          result: 'ok'
        }
      }
    },
    {
      event_type: 'assistant_final',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-3',
      event_seq: 3,
      user_turn_id: 'turn-1',
      model_turn_id: 'model-turn-1',
      message_id: 'message-assistant-1',
      content: 'done'
    }
  ]);

  const materialized = materializeChatRuntimeMessages(projection, 'session-1');
  const workflowItems = materialized[0].workflowItems as Array<Record<string, unknown>>;

  assert.equal(materialized.length, 1);
  assert.equal(materialized[0].workflowStreaming, false);
  assert.equal(workflowItems.length, 1);
  assert.equal(workflowItems[0].eventType, 'tool_result');
  assert.equal(workflowItems[0].status, 'completed');
  assert.equal(workflowItems[0].toolCallId, 'call-1');
  assert.equal(workflowItems[0].toolName, 'lookup');
});

test('chat runtime render adapter settles queued workflow artifacts when final arrives directly', () => {
  const projection = apply([
    {
      event_type: 'queue_status',
      source: 'test',
      strict: true,
      session_id: 'session-terminal-queue',
      event_id: 'event-queue-1',
      event_seq: 1,
      user_turn_id: 'turn-terminal-queue',
      model_turn_id: 'model-turn-terminal-queue',
      message_id: 'message-terminal-queue',
      payload: {
        source_event_type: 'queued',
        queue_position: 1
      }
    },
    {
      event_type: 'tool_call_started',
      source: 'test',
      strict: true,
      session_id: 'session-terminal-queue',
      event_id: 'event-tool-2',
      event_seq: 2,
      user_turn_id: 'turn-terminal-queue',
      model_turn_id: 'model-turn-terminal-queue',
      message_id: 'message-terminal-queue',
      payload: {
        source_event_type: 'tool_call',
        data: {
          tool_call_id: 'call-terminal-queue',
          tool: 'lookup'
        }
      }
    },
    {
      event_type: 'assistant_final',
      source: 'test',
      strict: true,
      session_id: 'session-terminal-queue',
      event_id: 'event-final-3',
      event_seq: 3,
      user_turn_id: 'turn-terminal-queue',
      model_turn_id: 'model-turn-terminal-queue',
      message_id: 'message-terminal-queue',
      content: '# Final response'
    }
  ]);

  const message = materializeChatRuntimeMessages(projection, 'session-terminal-queue')[0];
  const workflowItems = message.workflowItems as Array<Record<string, unknown>>;

  assert.equal(message.runtime_status, 'final');
  assert.equal(message.state, 'done');
  assert.equal(message.final, true);
  assert.equal(message.failed, false);
  assert.equal(message.cancelled, false);
  assert.equal(message.stream_incomplete, false);
  assert.equal(message.workflowStreaming, false);
  assert.equal(message.reasoningStreaming, false);
  assert.equal(workflowItems.some((item) => item.status === 'loading' || item.status === 'running'), false);
});

test('chat runtime render adapter overrides stale queued display fields with terminal projection state', () => {
  const message = materializeChatRuntimeMessage({
    id: 'message-terminal-display',
    role: 'assistant',
    content: '# Final response',
    reasoning: '',
    status: 'final',
    createdAt: '2026-04-30T02:14:07.000Z',
    createdSeq: 1,
    updatedSeq: 3,
    userTurnId: 'turn-terminal-display',
    modelTurnId: 'model-turn-terminal-display',
    final: true,
    failed: false,
    cancelled: false,
    display: {
      status: 'queued',
      state: 'queued',
      final: false,
      stream_incomplete: true,
      workflowStreaming: true
    },
    workflowItems: [{ eventType: 'queued', status: 'queued' }]
  });

  assert.ok(message);
  assert.equal(message.status, 'final');
  assert.equal(message.runtime_status, 'final');
  assert.equal(message.state, 'done');
  assert.equal(message.final, true);
  assert.equal(message.stream_incomplete, false);
  assert.equal(message.workflowStreaming, false);
  assert.equal((message.workflowItems as Array<Record<string, unknown>>)[0].status, 'completed');
  assert.equal(resolveAssistantMessageRuntimeState(message), 'done');
});

test('chat runtime render adapter materializes projected retry and resumable fields', () => {
  const projection = apply([
    {
      event_type: 'workflow_event',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-1',
      event_seq: 1,
      user_turn_id: 'turn-1',
      model_turn_id: 'model-turn-1',
      message_id: 'message-assistant-1',
      payload: {
        source_event_type: 'llm_stream_retry',
        data: {
          attempt: 1,
          max_attempts: 3,
          delay_s: 2,
          retry_reason: 'rate_limit',
          timestamp: '2026-04-30T02:14:07.000Z'
        }
      }
    },
    {
      event_type: 'workflow_event',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-2',
      event_seq: 2,
      user_turn_id: 'turn-1',
      model_turn_id: 'model-turn-1',
      message_id: 'message-assistant-1',
      payload: {
        source_event_type: 'slow_client',
        data: {
          reason: 'queue_full_resume_required'
        }
      }
    }
  ]);

  const materialized = materializeChatRuntimeMessages(projection, 'session-1');
  const workflowItems = materialized[0].workflowItems as Array<Record<string, unknown>>;

  assert.equal(materialized.length, 1);
  assert.equal(materialized[0].slow_client, true);
  assert.equal(materialized[0].resume_available, true);
  assert.equal(materialized[0].workflowStreaming, false);
  assert.equal(materialized[0].stream_incomplete, false);
  assert.equal(materialized[0].retry_attempt, 1);
  assert.equal(materialized[0].retry_max_attempts, 3);
  assert.equal(materialized[0].retry_delay_s, 2);
  assert.equal(workflowItems.some((item) => item.eventType === 'llm_stream_retry'), true);
  assert.equal(workflowItems.some((item) => item.eventType === 'slow_client'), true);
});

test('chat runtime render adapter materializes projected plan and question panel fields', () => {
  const projection = apply([
    {
      event_type: 'workflow_event',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-1',
      event_seq: 1,
      user_turn_id: 'turn-1',
      model_turn_id: 'model-turn-1',
      message_id: 'message-assistant-1',
      payload: {
        source_event_type: 'plan_update',
        data: {
          explanation: 'planned route',
          steps: [{ step: 'collect input', status: 'in_progress' }]
        }
      }
    },
    {
      event_type: 'workflow_event',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-2',
      event_seq: 2,
      user_turn_id: 'turn-1',
      model_turn_id: 'model-turn-1',
      message_id: 'message-assistant-1',
      payload: {
        source_event_type: 'question_panel',
        data: {
          question: 'Pick a route',
          routes: [{ label: 'Fast', recommended: true }]
        }
      }
    }
  ]);

  const materialized = materializeChatRuntimeMessages(projection, 'session-1');
  const message = materialized[0] as {
    plan?: { explanation?: string; steps?: Array<{ step?: string; status?: string }> };
    questionPanel?: { question?: string; routes?: Array<{ label?: string; recommended?: boolean }> };
    workflowItems?: Array<Record<string, unknown>>;
  };

  assert.equal(materialized.length, 1);
  assert.equal(message.plan?.explanation, 'planned route');
  assert.equal(message.plan?.steps?.[0]?.step, 'collect input');
  assert.equal(message.questionPanel?.question, 'Pick a route');
  assert.equal(message.questionPanel?.routes?.[0]?.recommended, true);
  assert.equal(message.workflowItems?.some((item) => item.eventType === 'plan_update'), true);
  assert.equal(message.workflowItems?.some((item) => item.eventType === 'question_panel'), true);
});

test('chat runtime render adapter materializes projected usage stats for message utilities', () => {
  const projection = apply([
    {
      event_type: 'assistant_message_created',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-stats-1',
      event_seq: 1,
      user_turn_id: 'turn-stats-1',
      model_turn_id: 'model-turn-stats-1',
      message_id: 'message-assistant-stats-1'
    },
    {
      event_type: 'usage_stats',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-stats-2',
      event_seq: 2,
      user_turn_id: 'turn-stats-1',
      model_turn_id: 'model-turn-stats-1',
      message_id: 'message-assistant-stats-1',
      payload: {
        source_event_type: 'round_usage',
        data: {
          input_tokens: 120,
          output_tokens: 30,
          total_tokens: 150,
          request_consumed_tokens: 150,
          context_occupancy_tokens: 180,
          max_context: 1000,
          decode_duration_s: 3,
          avg_model_round_speed_tps: 10,
          avg_model_round_speed_rounds: 1
        }
      }
    },
    {
      event_type: 'assistant_final',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-stats-3',
      event_seq: 3,
      user_turn_id: 'turn-stats-1',
      model_turn_id: 'model-turn-stats-1',
      message_id: 'message-assistant-stats-1',
      content: 'done'
    }
  ]);

  const materialized = materializeChatRuntimeMessages(projection, 'session-1');
  const message = materialized[0] as Record<string, any>;
  const entries = buildAssistantMessageStatsEntries(message, t, materialized);
  const contextSource = resolveComposerContextUsageSource(materialized, {
    id: 'session-1',
    context_occupancy_tokens: 180,
    context_max_tokens: 1000
  }, false);

  assert.equal(materialized.length, 1);
  assert.equal(message.__runtime_projected, true);
  assert.deepEqual(message.stats.roundUsage, { input: 120, output: 30, total: 150 });
  assert.equal(message.stats.context_occupancy_tokens, 180);
  assert.equal(message.stats.contextTotalTokens, 1000);
  assert.equal(message.stats.quotaConsumed, 150);
  assert.equal(entries.find((item) => item.key === 'contextTokens')?.value, '180');
  assert.equal(entries.find((item) => item.key === 'quota')?.value, '150');
  assert.equal(entries.find((item) => item.key === 'speed')?.value, '10.00 token/s');
  assert.equal(contextSource.contextTokens, 180);
  assert.equal(contextSource.contextTotalTokens, 1000);
  (message.stats.roundUsage as Record<string, unknown>).total = 1;
  const second = materializeChatRuntimeMessages(projection, 'session-1')[0] as Record<string, any>;
  assert.equal(second.stats.roundUsage.total, 150);
});

test('chat runtime render adapter updates cached materialization when projected stats mutate in place', () => {
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(projection, {
    event_type: 'assistant_message_created',
    source: 'test',
    strict: false,
    session_id: 'session-1',
    event_id: 'event-cache-stats-1',
    user_turn_id: 'turn-stats-cache-1',
    model_turn_id: 'model-turn-stats-cache-1',
    message_id: 'message-assistant-stats-cache-1'
  });

  const first = materializeChatRuntimeMessages(projection, 'session-1')[0] as Record<string, any>;
  assert.equal(first.stats?.avg_model_round_speed_tps ?? null, null);

  applyChatRuntimeEvent(projection, {
    event_type: 'tool_call_completed',
    source: 'test',
    strict: false,
    session_id: 'session-1',
    event_id: 'event-cache-stats-tool',
    user_turn_id: 'turn-stats-cache-1',
    model_turn_id: 'model-turn-stats-cache-1',
    message_id: 'message-assistant-stats-cache-1',
    payload: {
      source_event_type: 'tool_result',
      data: {
        tool_call_id: 'call-1',
        tool: 'lookup'
      }
    }
  });
  applyChatRuntimeEvent(projection, {
    event_type: 'usage_stats',
    source: 'test',
    strict: false,
    session_id: 'session-1',
    event_id: 'event-cache-stats-2',
    user_turn_id: 'turn-stats-cache-1',
    model_turn_id: 'model-turn-stats-cache-1',
    message_id: 'message-assistant-stats-cache-1',
    payload: {
      source_event_type: 'round_usage',
      data: {
        input_tokens: 10,
        output_tokens: 5,
        total_tokens: 15,
        request_consumed_tokens: 15,
        context_occupancy_tokens: 20,
        decode_duration_s: 1,
        avg_model_round_speed_tps: 5,
        avg_model_round_speed_rounds: 1
      }
    }
  });
  applyChatRuntimeEvent(projection, {
    event_type: 'assistant_final',
    source: 'test',
    strict: false,
    session_id: 'session-1',
    event_id: 'event-cache-stats-3',
    user_turn_id: 'turn-stats-cache-1',
    model_turn_id: 'model-turn-stats-cache-1',
    message_id: 'message-assistant-stats-cache-1',
    content: 'done'
  });

  const second = materializeChatRuntimeMessages(projection, 'session-1')[0] as Record<string, any>;
  const entries = buildAssistantMessageStatsEntries(second, t, [second]);

  assert.equal(second, first);
  assert.equal(second.stats.avg_model_round_speed_tps, 5);
  assert.equal(second.stats.toolCalls, 1);
  assert.equal(entries.find((item) => item.key === 'speed')?.value, '5.00 token/s');
  assert.equal(entries.find((item) => item.key === 'toolCalls')?.value, '1');
});

test('chat runtime render adapter preserves projected subagent cards', () => {
  const projection = apply([
    {
      event_type: 'workflow_event',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-1',
      event_seq: 1,
      user_turn_id: 'turn-1',
      model_turn_id: 'model-turn-1',
      message_id: 'message-assistant-1',
      payload: {
        source_event_type: 'subagent_dispatch_item_update',
        data: {
          session_id: 'child-session-1',
          run_id: 'child-run-1',
          label: 'Worker A',
          status: 'running',
          summary: 'started'
        }
      }
    }
  ]);

  const materialized = materializeChatRuntimeMessages(projection, 'session-1');
  const workflowItems = materialized[0].workflowItems as Array<Record<string, unknown>>;
  const subagents = materialized[0].subagents as Array<Record<string, unknown>>;

  assert.equal(materialized.length, 1);
  assert.equal(materialized[0].workflowStreaming, true);
  assert.equal(workflowItems.length, 1);
  assert.equal(workflowItems[0].kind, 'subagent');
  assert.equal(subagents.length, 1);
  assert.equal(subagents[0].key, 'child-run-1');
  assert.equal(subagents[0].status, 'running');
  assert.equal(subagents[0].terminal, false);
});

test('chat runtime render adapter marks completed subagents as non-streaming after terminal turn', () => {
  const projection = apply([
    {
      event_type: 'workflow_event',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-1',
      event_seq: 1,
      user_turn_id: 'turn-1',
      model_turn_id: 'model-turn-1',
      message_id: 'message-assistant-1',
      payload: {
        source_event_type: 'subagent_dispatch_item_update',
        data: {
          session_id: 'child-session-1',
          run_id: 'child-run-1',
          label: 'Worker A',
          status: 'running'
        }
      }
    },
    {
      event_type: 'turn_completed',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-2',
      event_seq: 2,
      user_turn_id: 'turn-1',
      model_turn_id: 'model-turn-1'
    }
  ]);

  const materialized = materializeChatRuntimeMessages(projection, 'session-1');
  const subagents = materialized[0].subagents as Array<Record<string, unknown>>;

  assert.equal(materialized[0].workflowStreaming, false);
  assert.equal(subagents.length, 1);
  assert.equal(subagents[0].status, 'completed');
  assert.equal(subagents[0].terminal, true);
  assert.equal(subagents[0].canTerminate, false);
});

test('chat runtime render adapter keeps workflow streaming for active subagent-only legacy snapshots', () => {
  const projection = apply([
    {
      event_type: 'session_snapshot',
      source: 'snapshot',
      strict: false,
      session_id: 'session-1',
      messages: [
        {
          message_id: 'message-assistant-1',
          role: 'assistant',
          content: '',
          workflowStreaming: false,
          stream_incomplete: false,
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
    }
  ]);

  const materialized = materializeChatRuntimeMessages(projection, 'session-1');

  assert.equal(materialized.length, 1);
  assert.equal(materialized[0].workflowStreaming, true);
  assert.equal(materialized[0].stream_incomplete, true);
});

test('chat runtime render adapter returns empty render list for empty projection', () => {
  const projection = createChatRuntimeProjection();
  const renderable = buildChatRuntimeRenderableMessages({
    projection,
    sessionId: 'missing-session'
  });

  assert.deepEqual(renderable, []);
  assert.equal(hasChatRuntimeRenderSession(projection, 'missing-session'), false);
});

test('chat runtime render adapter reports known empty projection sessions', () => {
  const projection = apply([
    {
      event_type: 'session_runtime',
      source: 'test',
      strict: true,
      session_id: 'session-empty',
      event_id: 'event-1',
      event_seq: 1,
      payload: {
        status: 'idle'
      }
    }
  ]);
  const renderable = buildChatRuntimeRenderableMessages({
    projection,
    sessionId: 'session-empty'
  });

  assert.deepEqual(renderable, []);
  assert.equal(hasChatRuntimeRenderSession(projection, 'session-empty'), true);
});

test('chat runtime render mode always keeps projection as the rendered source', () => {
  withWindowFlags({}, () => {
    assert.equal(resolveChatRuntimeProjectionRenderMode(), 'projection');
    assert.equal(isChatRuntimeProjectionRenderEnabled(), true);
  });
  withWindowFlags({ 'wunder:chat-runtime-render': 'legacy' }, () => {
    assert.equal(resolveChatRuntimeProjectionRenderMode(), 'projection');
    assert.equal(isChatRuntimeProjectionRenderEnabled(), true);
  });
  withWindowFlags({ 'wunder:chat-runtime-render': 'projection' }, () => {
    assert.equal(resolveChatRuntimeProjectionRenderMode(), 'shadow');
    assert.equal(isChatRuntimeProjectionRenderEnabled(), false);
  });
  withWindowFlags({ 'wunder:chat-runtime-render': 'projection-debug' }, () => {
    assert.equal(resolveChatRuntimeProjectionRenderMode(), 'projection');
    assert.equal(isChatRuntimeProjectionRenderEnabled(), true);
  });
  withWindowFlags({}, () => {
    assert.equal(resolveChatRuntimeProjectionRenderMode(), 'projection');
  }, '?chat_runtime_render=off');
});

test('chat runtime render source decision ignores legacy rollback requests', () => {
  assert.deepEqual(
    resolveChatRuntimeRenderableSourceDecision({
      renderMode: 'projection',
      projectionCount: 3,
      projectionSessionKnown: true,
      shadowEnabled: false
    }),
    {
      source: 'projection',
      event: 'projection-source',
      inspectShadow: false
    }
  );
});

test('chat runtime render source decision observes projection in shadow mode without switching source', () => {
  assert.deepEqual(
    resolveChatRuntimeRenderableSourceDecision({
      renderMode: 'shadow',
      projectionCount: 2,
      projectionSessionKnown: true,
      shadowEnabled: true
    }),
    {
      source: 'projection',
      event: 'projection-source',
      inspectShadow: true
    }
  );
});

test('chat runtime render source decision renders known empty projection sessions', () => {
  assert.deepEqual(
    resolveChatRuntimeRenderableSourceDecision({
      renderMode: 'projection',
      projectionCount: 0,
      projectionSessionKnown: true,
      shadowEnabled: false
    }),
    {
      source: 'projection',
      event: 'projection-source',
      inspectShadow: false
    }
  );
});

test('chat runtime render source decision keeps projection for unknown empty sessions', () => {
  assert.deepEqual(
    resolveChatRuntimeRenderableSourceDecision({
      renderMode: 'projection',
      projectionCount: 0,
      projectionSessionKnown: false,
      shadowEnabled: false
    }),
    {
      source: 'projection',
      event: 'projection-source',
      inspectShadow: false
    }
  );
});
