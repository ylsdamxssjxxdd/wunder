import test from 'node:test';
import assert from 'node:assert/strict';
import { computed, reactive } from 'vue';

import {
  resolveRuntimeMessageContentSource,
  resolveRuntimeMessageContentSubscriptionIds
} from '../../src/components/chat/messageRuntimeContent';
import {
  createChatRuntimeProjection,
  createChatRuntimeSessionProjection
} from '../../src/realtime/chat/chatRuntimeReducer';

test('message content keeps the exact rendered message when a stale model turn points elsewhere', () => {
  const sessionId = 'session-runtime-content';
  const oldMessageId = 'assistant:initial-fragment';
  const finalMessageId = 'assistant:final-response';
  const staleModelTurnId = 'model-turn:stale';
  const finalModelTurnId = 'model-turn:final';
  const userTurnId = 'user-turn:1';
  const projection = createChatRuntimeProjection();
  const session = createChatRuntimeSessionProjection(sessionId);
  projection.sessions[sessionId] = session;

  session.messages = [oldMessageId, finalMessageId];
  session.messageById[oldMessageId] = {
    id: oldMessageId,
    role: 'assistant',
    content: 'short fragment',
    reasoning: '',
    status: 'final',
    createdAt: '2026-01-01T00:00:00.000Z',
    createdSeq: 1,
    updatedSeq: 1,
    userTurnId,
    modelTurnId: staleModelTurnId,
    final: true,
    failed: false,
    cancelled: false
  };
  session.messageById[finalMessageId] = {
    id: finalMessageId,
    role: 'assistant',
    content: 'complete final response preserved after the tool sequence',
    reasoning: '',
    status: 'final',
    createdAt: '2026-01-01T00:00:01.000Z',
    createdSeq: 2,
    updatedSeq: 2,
    userTurnId,
    modelTurnId: finalModelTurnId,
    final: true,
    failed: false,
    cancelled: false
  };
  session.modelTurnById[staleModelTurnId] = {
    id: staleModelTurnId,
    userTurnId,
    createdSeq: 1,
    messageIds: [oldMessageId],
    finalMessageId: oldMessageId,
    status: 'completed'
  };
  session.modelTurnById[finalModelTurnId] = {
    id: finalModelTurnId,
    userTurnId,
    createdSeq: 2,
    messageIds: [finalMessageId],
    finalMessageId,
    status: 'completed'
  };
  session.userTurnById[userTurnId] = {
    id: userTurnId,
    createdSeq: 1,
    messageIds: [],
    modelTurnIds: [staleModelTurnId, finalModelTurnId],
    status: 'completed'
  };

  const options = {
    projection,
    sessionId,
    runtimeMessageId: finalMessageId,
    // This represents an older turn id that can survive an in-place merge.
    runtimeModelTurnId: staleModelTurnId,
    runtimeUserTurnId: userTurnId,
    message: { role: 'assistant' }
  };

  assert.equal(resolveRuntimeMessageContentSource(options)?.id, finalMessageId);
  assert.equal(
    resolveRuntimeMessageContentSource(options)?.content,
    'complete final response preserved after the tool sequence'
  );
  assert.deepEqual(resolveRuntimeMessageContentSubscriptionIds(options), [finalMessageId]);
});

test('message content uses the model turn only when the exact message is unavailable', () => {
  const sessionId = 'session-runtime-content-fallback';
  const messageId = 'assistant:fallback';
  const modelTurnId = 'model-turn:fallback';
  const projection = createChatRuntimeProjection();
  const session = createChatRuntimeSessionProjection(sessionId);
  projection.sessions[sessionId] = session;
  session.messageById[messageId] = {
    id: messageId,
    role: 'assistant',
    content: 'fallback response',
    reasoning: '',
    status: 'final',
    createdAt: '2026-01-01T00:00:00.000Z',
    createdSeq: 1,
    updatedSeq: 1,
    userTurnId: 'user-turn:fallback',
    modelTurnId,
    final: true,
    failed: false,
    cancelled: false
  };
  session.modelTurnById[modelTurnId] = {
    id: modelTurnId,
    userTurnId: 'user-turn:fallback',
    createdSeq: 1,
    messageIds: [messageId],
    finalMessageId: messageId,
    status: 'completed'
  };

  const options = {
    projection,
    sessionId,
    runtimeMessageId: 'assistant:not-yet-indexed',
    runtimeModelTurnId: modelTurnId,
    message: { role: 'assistant' }
  };
  assert.equal(resolveRuntimeMessageContentSource(options)?.id, messageId);
  assert.deepEqual(resolveRuntimeMessageContentSubscriptionIds(options), [messageId]);
});

test('workflow history resolves through a rendered turn identity while its materialized row is empty', () => {
  const sessionId = 'session-runtime-workflow-history';
  const messageId = 'assistant:workflow-history';
  const userTurnId = 'user-turn:workflow-history';
  const modelTurnId = 'model-turn:workflow-history';
  const projection = createChatRuntimeProjection();
  const session = createChatRuntimeSessionProjection(sessionId);
  projection.sessions[sessionId] = session;
  session.messageById[messageId] = {
    id: messageId,
    role: 'assistant',
    content: 'completed response',
    reasoning: '',
    status: 'final',
    createdAt: '2026-01-01T00:00:00.000Z',
    createdSeq: 1,
    updatedSeq: 1,
    userTurnId,
    modelTurnId,
    final: true,
    failed: false,
    cancelled: false,
    workflowItems: [{
      id: 'tool:history',
      eventType: 'tool_result',
      status: 'completed'
    }],
    subagents: [{
      id: 'subagent:history',
      status: 'completed'
    }]
  };
  session.modelTurnById[modelTurnId] = {
    id: modelTurnId,
    userTurnId,
    createdSeq: 1,
    messageIds: [messageId],
    finalMessageId: messageId,
    status: 'completed'
  };
  session.userTurnById[userTurnId] = {
    id: userTurnId,
    createdSeq: 1,
    messageIds: [],
    modelTurnIds: [modelTurnId],
    status: 'completed'
  };

  const projected = resolveRuntimeMessageContentSource({
    projection,
    sessionId,
    // The view can temporarily carry an obsolete message id while its turn
    // identity is already stable during refresh hydration.
    runtimeMessageId: 'assistant:materializing',
    runtimeUserTurnId: userTurnId,
    runtimeModelTurnId: modelTurnId,
    message: { role: 'assistant' }
  });

  assert.equal(projected?.id, messageId);
  assert.equal(projected?.workflowItems?.length, 1);
  assert.equal(projected?.subagents?.length, 1);
});

test('workflow history resolves from turn identity when refresh has not assigned a message id yet', () => {
  const sessionId = 'session-runtime-workflow-turn-only';
  const messageId = 'assistant:workflow-turn-only';
  const userTurnId = 'user-turn:workflow-turn-only';
  const modelTurnId = 'model-turn:workflow-turn-only';
  const projection = createChatRuntimeProjection();
  const session = createChatRuntimeSessionProjection(sessionId);
  projection.sessions[sessionId] = session;
  session.messageById[messageId] = {
    id: messageId,
    role: 'assistant',
    content: '',
    reasoning: '',
    status: 'final',
    createdAt: '2026-01-01T00:00:00.000Z',
    createdSeq: 1,
    updatedSeq: 1,
    userTurnId,
    modelTurnId,
    final: true,
    failed: false,
    cancelled: false,
    workflowItems: [{ id: 'tool:turn-only', eventType: 'tool_result', status: 'completed' }]
  };
  session.modelTurnById[modelTurnId] = {
    id: modelTurnId,
    userTurnId,
    createdSeq: 1,
    messageIds: [messageId],
    finalMessageId: messageId,
    status: 'completed'
  };

  const projected = resolveRuntimeMessageContentSource({
    projection,
    sessionId,
    runtimeUserTurnId: userTurnId,
    runtimeModelTurnId: modelTurnId,
    message: { role: 'assistant' }
  });

  assert.equal(projected?.id, messageId);
  assert.equal(projected?.workflowItems?.[0]?.id, 'tool:turn-only');
});

test('message content tracks the exact runtime message through its final replacement', () => {
  const sessionId = 'session-runtime-content-reactive';
  const messageId = 'assistant:reactive-final';
  const projection = reactive(createChatRuntimeProjection());
  const session = createChatRuntimeSessionProjection(sessionId);
  projection.sessions[sessionId] = session;
  session.messageById[messageId] = {
    id: messageId,
    role: 'assistant',
    content: 'initial fragment',
    reasoning: '',
    status: 'streaming',
    createdAt: '2026-01-01T00:00:00.000Z',
    createdSeq: 1,
    updatedSeq: 1,
    userTurnId: 'user-turn:reactive',
    modelTurnId: 'model-turn:reactive',
    final: false,
    failed: false,
    cancelled: false
  };

  const options = {
    projection,
    sessionId,
    runtimeMessageId: messageId,
    message: { role: 'assistant' }
  };
  const displayed = computed(() => {
    const message = resolveRuntimeMessageContentSource(options);
    return `${message?.status || ''}:${message?.content || ''}`;
  });

  assert.equal(displayed.value, 'streaming:initial fragment');
  projection.sessions[sessionId].messageById[messageId].content = 'complete final response';
  projection.sessions[sessionId].messageById[messageId].status = 'final';
  assert.equal(displayed.value, 'final:complete final response');
});
