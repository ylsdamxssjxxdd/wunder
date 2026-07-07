import test from 'node:test';
import assert from 'node:assert/strict';

import { createChatRuntimeProjection, applyChatRuntimeEvent } from '../../src/realtime/chat/chatRuntimeReducer';
import {
  buildChatRuntimeRenderableMessages,
  hasChatRuntimeRenderSession,
  isChatRuntimeProjectionRenderEnabled,
  materializeChatRuntimeMessages,
  resolveChatRuntimeProjectionRenderMode,
  resolveChatRuntimeRenderableSourceDecision,
  resolveChatRuntimeMessageRenderKey
} from '../../src/realtime/chat/chatRuntimeRenderAdapter';
import { resolveComposerContextUsageSource } from '../../src/components/chat/composerContextUsage';
import { buildAssistantMessageStatsEntries } from '../../src/utils/messageStats';
import type { ChatRuntimeEvent } from '../../src/realtime/chat/chatRuntimeTypes';

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
    'messenger.messageStatus.modelOutputting': 'Outputting'
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

test('chat runtime render adapter invalidates cached materialization when projected stats mutate in place', () => {
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

  assert.notEqual(second, first);
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
