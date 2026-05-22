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

test('chat runtime render adapter preserves compatible legacy raw message references', () => {
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
      event_type: 'legacy_messages_reconciled',
      source: 'legacy',
      strict: false,
      session_id: 'session-1',
      messages: [user, assistant],
      loading: false,
      running: false
    }
  ]);

  const materialized = materializeChatRuntimeMessages(projection, 'session-1');

  assert.equal(materialized[0], user);
  assert.equal(materialized[1], assistant);
  assert.deepEqual(materialized[0].attachments, [{ name: 'file' }]);
  assert.deepEqual(materialized[1].feedback, { vote: 'up' });
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
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
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
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
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
      event_type: 'legacy_messages_reconciled',
      source: 'legacy',
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

test('chat runtime render mode defaults to legacy and requires debug projection opt-in', () => {
  withWindowFlags({}, () => {
    assert.equal(resolveChatRuntimeProjectionRenderMode(), 'legacy');
    assert.equal(isChatRuntimeProjectionRenderEnabled(), false);
  });
  withWindowFlags({ 'wunder:chat-runtime-render': 'legacy' }, () => {
    assert.equal(resolveChatRuntimeProjectionRenderMode(), 'legacy');
    assert.equal(isChatRuntimeProjectionRenderEnabled(), false);
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
    assert.equal(resolveChatRuntimeProjectionRenderMode(), 'legacy');
  }, '?chat_runtime_render=off');
});

test('chat runtime render source decision keeps legacy only when rollback mode is explicit', () => {
  assert.deepEqual(
    resolveChatRuntimeRenderableSourceDecision({
      renderMode: 'legacy',
      projectionCount: 3,
      projectionSessionKnown: true,
      shadowEnabled: false
    }),
    {
      source: 'legacy',
      event: 'legacy-source',
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
      source: 'legacy',
      event: 'projection-shadow',
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
      inspectShadow: true
    }
  );
});

test('chat runtime render source decision falls back only for unknown empty projection sessions', () => {
  assert.deepEqual(
    resolveChatRuntimeRenderableSourceDecision({
      renderMode: 'projection',
      projectionCount: 0,
      projectionSessionKnown: false,
      shadowEnabled: false
    }),
    {
      source: 'legacy',
      event: 'projection-empty-fallback',
      inspectShadow: true
    }
  );
});
