import test from 'node:test';
import assert from 'node:assert/strict';

import type {
  ChatThreadFrame,
  ChatThreadState,
  ThreadChangeFrame,
  ThreadItemTailFrame
} from '../../src/realtime/chat/chatThreadTypes';
import { emptyChatThreadState } from '../../src/realtime/chat/chatThreadTypes';
import { applyChatThreadFrame } from '../../src/realtime/chat/chatThreadState';
import { buildChatThreadRenderableMessages } from '../../src/realtime/chat/chatThreadProjection';
import { materializeChatRuntimeMessage } from '../../src/realtime/chat/chatRuntimeRenderAdapter';

type TestFrame = ChatThreadFrame;

const itemUpsert = (seq: number, data: Record<string, unknown>): ThreadChangeFrame => {
  const frame: ThreadChangeFrame = { event: 'thread_change', seq, change_type: 'item_upsert', data };
  if (typeof data.item_id === 'string') frame.item_id = data.item_id;
  if (typeof data.turn_id === 'string') frame.turn_id = data.turn_id;
  return frame;
};

const turnUpsert = (seq: number, data: Record<string, unknown>): ThreadChangeFrame => {
  const frame: ThreadChangeFrame = { event: 'thread_change', seq, change_type: 'turn_upsert', data };
  if (typeof data.turn_id === 'string') frame.turn_id = data.turn_id;
  return frame;
};

const turnStatus = (seq: number, turnId: string, status: string): ThreadChangeFrame => ({
  event: 'thread_change',
  seq,
  change_type: 'turn_status',
  turn_id: turnId,
  data: { turn_id: turnId, status }
});

const tailFrame = (itemId: string, field: string, offset: number, text: string): ThreadItemTailFrame => ({
  event: 'thread_item_tail',
  item_id: itemId,
  field,
  offset,
  text
});

const applyFrames = (
  state: ChatThreadState,
  frames: TestFrame[]
): void => {
  frames.forEach((frame, index) => applyChatThreadFrame(state, frame, index));
};

const textItemData = (
  turnId: string,
  round: number,
  extra: Record<string, unknown> = {}
): Record<string, unknown> => ({
  item_id: `${turnId}:text-${round}`,
  turn_id: turnId,
  model_round: round,
  kind: 'assistant_message',
  role: 'assistant',
  visibility: 'user',
  status: 'completed',
  revision: 1,
  ...extra
});

const toolItemData = (
  turnId: string,
  callId: string,
  extra: Record<string, unknown> = {}
): Record<string, unknown> => ({
  item_id: `${turnId}:tool-${callId}`,
  turn_id: turnId,
  kind: 'tool_call',
  visibility: 'user',
  status: 'running',
  revision: 1,
  tool_call_id: callId,
  ...extra
});

const workflowIds = (message: { workflowItems?: unknown } | null | undefined): unknown[] => {
  const items = message?.workflowItems;
  return Array.isArray(items)
    ? items.map((item) => (item as Record<string, unknown>).id)
    : [];
};

/** Registration-order independent shape: identity, text, status, cards. */
const projectionShape = (messages: ReturnType<typeof buildChatThreadRenderableMessages>) =>
  messages.map((message) => ({
    id: message.id,
    role: message.role,
    content: message.content,
    reasoning: message.reasoning,
    status: message.status,
    final: message.final,
    failed: message.failed,
    cancelled: message.cancelled,
    userTurnId: message.userTurnId,
    modelTurnId: message.modelTurnId,
    workflowIds: workflowIds(message)
  }));

test('chat thread projection emits one bubble per model round in order', () => {
  const state = emptyChatThreadState('session-projection-rounds');
  applyFrames(state, [
    turnUpsert(1, { turn_id: 'turn-1', user_round: 0, status: 'running', content: 'question' }),
    itemUpsert(2, textItemData('turn-1', 0, { content: 'round zero' })),
    itemUpsert(3, textItemData('turn-1', 1, { status: 'running', content: '' })),
    tailFrame('turn-1:text-1', 'content', 0, 'round one')
  ]);

  const messages = buildChatThreadRenderableMessages(state);

  assert.deepEqual(messages.map((message) => message.id), [
    'tturn:turn-1:user',
    'titem:turn-1:text-0',
    'titem:turn-1:text-1'
  ]);
  assert.deepEqual(messages.map((message) => message.content), [
    'question',
    'round zero',
    'round one'
  ]);
  // Two separate bubbles: the later round never overwrites the earlier one.
  assert.equal(messages[1].role, 'assistant');
  assert.equal(messages[2].role, 'assistant');
  assert.notEqual(messages[1].id, messages[2].id);
  assert.equal(messages[1].userTurnId, 'turn-1');
  assert.equal(messages[1].modelTurnId, 'turn-1:round-0');
  assert.equal(messages[2].modelTurnId, 'turn-1:round-1');
});

test('chat thread projection renders the user bubble from turn content', () => {
  const state = emptyChatThreadState('session-projection-user');
  applyFrames(state, [
    turnUpsert(1, { turn_id: 'turn-1', user_round: 3, status: 'running', content: 'please help' }),
    itemUpsert(2, textItemData('turn-1', 0, { content: 'sure' }))
  ]);

  const messages = buildChatThreadRenderableMessages(state);

  assert.equal(messages.length, 2);
  const user = messages[0];
  assert.equal(user.id, 'tturn:turn-1:user');
  assert.equal(user.role, 'user');
  assert.equal(user.content, 'please help');
  assert.equal(user.userTurnId, 'turn-1');
  assert.equal(user.modelTurnId, '');
  assert.equal(user.status, 'final');
  assert.equal(user.final, true);
  assert.equal(user.createdSeq, 0);
  assert.ok(messages[1].createdSeq >= user.createdSeq);
});

test('chat thread projection composes blocks plus tail and propagates turn completion', () => {
  const state = emptyChatThreadState('session-projection-terminal');
  applyFrames(state, [
    turnUpsert(1, { turn_id: 'turn-1', user_round: 0, status: 'running', content: 'hi' }),
    itemUpsert(2, textItemData('turn-1', 0, { status: 'running', revision: 1, content: '', reasoning: '' })),
    {
      event: 'thread_change',
      seq: 3,
      change_type: 'text_block',
      item_id: 'turn-1:text-0',
      data: { item_id: 'turn-1:text-0', field: 'content', block_index: 0, content_offset: 0, content: 'Hello' }
    },
    tailFrame('turn-1:text-0', 'content', 5, ' world'),
    {
      event: 'thread_change',
      seq: 4,
      change_type: 'text_block',
      item_id: 'turn-1:text-0',
      data: { item_id: 'turn-1:text-0', field: 'reasoning', block_index: 0, reasoning_offset: 0, reasoning: 'thinking' }
    },
    tailFrame('turn-1:text-0', 'reasoning', 8, ' more')
  ]);

  const streaming = buildChatThreadRenderableMessages(state);
  const streamingBubble = streaming.find((message) => message.role === 'assistant');
  assert.ok(streamingBubble);
  assert.equal(streamingBubble.status, 'streaming');
  assert.equal(streamingBubble.final, false);
  assert.equal(streamingBubble.content, 'Hello world');
  assert.equal(streamingBubble.reasoning, 'thinking more');

  applyFrames(state, [
    itemUpsert(5, textItemData('turn-1', 0, { status: 'completed', revision: 2 })),
    turnStatus(6, 'turn-1', 'completed')
  ]);

  const terminal = buildChatThreadRenderableMessages(state);
  const terminalBubble = terminal.find((message) => message.role === 'assistant');
  assert.ok(terminalBubble);
  assert.equal(terminalBubble.status, 'final');
  assert.equal(terminalBubble.final, true);
  assert.equal(terminalBubble.failed, false);
  assert.equal(terminalBubble.cancelled, false);
  // Composed text survives the terminal upsert (durable blocks stay authoritative).
  assert.equal(terminalBubble.content, 'Hello world');
});

test('chat thread projection groups workflow items by model round and orphans into the last bubble', () => {
  const state = emptyChatThreadState('session-projection-workflow');
  applyFrames(state, [
    turnUpsert(1, { turn_id: 'turn-1', user_round: 0, status: 'running', content: 'q' }),
    itemUpsert(2, textItemData('turn-1', 0, { content: 'part one' })),
    itemUpsert(3, textItemData('turn-1', 1, { content: 'part two' })),
    itemUpsert(4, toolItemData('turn-1', 'call-a', {
      model_round: 0,
      event_type: 'tool_call',
      tool: 'lookup'
    })),
    itemUpsert(5, toolItemData('turn-1', 'call-b', {
      model_round: 1,
      event_type: 'tool_result',
      status: 'completed',
      tool: 'lookup',
      content: 'ok result'
    })),
    itemUpsert(6, {
      item_id: 'turn-1:approval-x',
      turn_id: 'turn-1',
      model_round: 1,
      kind: 'approval',
      visibility: 'user',
      status: 'running',
      revision: 1,
      event_type: 'approval_request',
      approval_id: 'approval-x',
      tool: 'deploy'
    }),
    // No model_round in the payload: must land in the turn's last bubble.
    itemUpsert(7, toolItemData('turn-1', 'call-c', {
      event_type: 'tool_result',
      status: 'completed',
      tool: 'lookup'
    }))
  ]);

  const messages = buildChatThreadRenderableMessages(state);
  const assistants = messages.filter((message) => message.role === 'assistant');
  assert.equal(assistants.length, 2);

  const roundZero = assistants[0];
  const roundOne = assistants[1];
  assert.deepEqual(workflowIds(roundZero), ['turn-1:tool-call-a']);
  // Rounded items in registration order, then the model_round-less item.
  assert.deepEqual(workflowIds(roundOne), [
    'turn-1:tool-call-b',
    'turn-1:approval-x',
    'turn-1:tool-call-c'
  ]);

  const callRecord = (roundZero.workflowItems as Array<Record<string, unknown>>)[0];
  assert.equal(callRecord.eventType, 'tool_call');
  assert.equal(callRecord.status, 'loading');
  assert.equal(callRecord.toolName, 'lookup');
  assert.equal(callRecord.toolCallId, 'call-a');
  assert.equal(callRecord.title, 'Tool call: lookup');
  assert.equal(callRecord.isTool, true);
  assert.equal(callRecord.modelTurnId, roundZero.modelTurnId);

  const resultRecord = (roundOne.workflowItems as Array<Record<string, unknown>>)[0];
  assert.equal(resultRecord.eventType, 'tool_result');
  assert.equal(resultRecord.status, 'completed');
  assert.equal(resultRecord.detail, 'ok result');
  assert.equal(resultRecord.title, 'Tool result: lookup');

  const approvalRecord = (roundOne.workflowItems as Array<Record<string, unknown>>)[1];
  assert.equal(approvalRecord.eventType, 'approval_request');
  assert.equal(approvalRecord.status, 'loading');
  assert.equal(approvalRecord.title, 'Approval required: deploy');

  // A live turn with running tools keeps the legacy tooling semantics.
  assert.equal(roundOne.status, 'tooling');
  assert.equal(roundZero.status, 'tooling');
});

test('chat thread projection keeps workflow-only rounds and standalone workflow turns', () => {
  const state = emptyChatThreadState('session-projection-standalone');
  applyFrames(state, [
    turnUpsert(1, { turn_id: 'turn-1', user_round: 0, status: 'running' }),
    // Rounded workflow with no text item yet: workflow-only bubble on that round.
    itemUpsert(2, toolItemData('turn-1', 'call-a', {
      model_round: 2,
      event_type: 'tool_call',
      tool: 'lookup'
    })),
    turnUpsert(3, { turn_id: 'turn-2', user_round: 1, status: 'running' }),
    // Orphan workflow with no text and no rounds anywhere: standalone bubble.
    itemUpsert(4, toolItemData('turn-2', 'call-b', {
      event_type: 'tool_call',
      tool: 'probe'
    }))
  ]);

  const messages = buildChatThreadRenderableMessages(state);
  const assistants = messages.filter((message) => message.role === 'assistant');

  assert.deepEqual(assistants.map((message) => message.id), [
    'titem:turn-1:text-2',
    'titem:turn-2:text-0'
  ]);
  assert.equal(assistants[0].content, '');
  assert.deepEqual(workflowIds(assistants[0]), ['turn-1:tool-call-a']);
  assert.equal(assistants[0].status, 'tooling');
  assert.deepEqual(workflowIds(assistants[1]), ['turn-2:tool-call-b']);
  assert.equal(assistants[1].status, 'tooling');
});

test('chat thread projection excludes admin and internal items', () => {
  const state = emptyChatThreadState('session-projection-visibility');
  applyFrames(state, [
    turnUpsert(1, { turn_id: 'turn-1', user_round: 0, status: 'running', content: 'q' }),
    itemUpsert(2, textItemData('turn-1', 0, { content: 'answer' })),
    // model_call items are admin-only diagnostics; never a bubble.
    itemUpsert(3, {
      item_id: 'turn-1:model-1',
      turn_id: 'turn-1',
      model_round: 1,
      kind: 'model_call',
      visibility: 'admin',
      status: 'running',
      revision: 1
    }),
    // Internal tool records never join the visible workflow cards.
    itemUpsert(4, toolItemData('turn-1', 'call-hidden', {
      model_round: 0,
      event_type: 'tool_call',
      tool: 'secret',
      visibility: 'model_internal'
    })),
    itemUpsert(5, toolItemData('turn-1', 'call-visible', {
      model_round: 0,
      event_type: 'tool_call',
      tool: 'lookup'
    }))
  ]);

  const messages = buildChatThreadRenderableMessages(state);
  const assistants = messages.filter((message) => message.role === 'assistant');

  assert.equal(assistants.length, 1);
  assert.equal(assistants[0].id, 'titem:turn-1:text-0');
  assert.deepEqual(workflowIds(assistants[0]), ['turn-1:tool-call-visible']);
});

test('chat thread projection is deterministic across item registration order', () => {
  const buildStateA = (): ChatThreadState => {
    const state = emptyChatThreadState('session-projection-determinism');
    applyFrames(state, [
      turnUpsert(1, { turn_id: 'turn-1', user_round: 0, status: 'running', content: 'q' }),
      itemUpsert(2, textItemData('turn-1', 0, { content: 'zero' })),
      itemUpsert(3, toolItemData('turn-1', 'call-a', { model_round: 0, event_type: 'tool_call', tool: 'lookup' })),
      itemUpsert(4, textItemData('turn-1', 1, { content: 'one' })),
      itemUpsert(5, toolItemData('turn-1', 'call-b', { model_round: 1, event_type: 'tool_result', status: 'completed', tool: 'lookup' })),
      itemUpsert(6, toolItemData('turn-1', 'call-c', { event_type: 'tool_result', status: 'completed', tool: 'lookup' })),
      turnUpsert(7, { turn_id: 'turn-2', user_round: null, status: 'completed' }),
      itemUpsert(8, textItemData('turn-2', 0, { content: 'later' }))
    ]);
    return state;
  };
  const buildStateB = (): ChatThreadState => {
    const state = emptyChatThreadState('session-projection-determinism');
    applyFrames(state, [
      itemUpsert(1, textItemData('turn-1', 1, { content: 'one' })),
      itemUpsert(2, toolItemData('turn-1', 'call-b', { model_round: 1, event_type: 'tool_result', status: 'completed', tool: 'lookup' })),
      itemUpsert(3, textItemData('turn-1', 0, { content: 'zero' })),
      itemUpsert(4, toolItemData('turn-1', 'call-a', { model_round: 0, event_type: 'tool_call', tool: 'lookup' })),
      turnUpsert(5, { turn_id: 'turn-1', user_round: 0, status: 'running', content: 'q' }),
      itemUpsert(6, toolItemData('turn-1', 'call-c', { event_type: 'tool_result', status: 'completed', tool: 'lookup' })),
      itemUpsert(7, textItemData('turn-2', 0, { content: 'later' })),
      turnUpsert(8, { turn_id: 'turn-2', user_round: null, status: 'completed' })
    ]);
    return state;
  };

  const first = buildChatThreadRenderableMessages(buildStateA());
  const second = buildChatThreadRenderableMessages(buildStateB());
  const replay = buildChatThreadRenderableMessages(buildStateA());

  // Round ordering defines bubbles; registration order must not reshuffle them.
  // (createdSeq mirrors ThreadItemState.order and legitimately differs.)
  assert.deepEqual(projectionShape(second), projectionShape(first));
  assert.deepEqual(projectionShape(replay), projectionShape(first));
  assert.deepEqual(first.map((message) => message.id), [
    'tturn:turn-1:user',
    'titem:turn-1:text-0',
    'titem:turn-1:text-1',
    'titem:turn-2:text-0'
  ]);
  // The null user_round turn sorts last deterministically.
  assert.equal(first[3].userTurnId, 'turn-2');
  assert.deepEqual(workflowIds(first[2]), ['turn-1:tool-call-b', 'turn-1:tool-call-c']);

  // Pure function: no cross-call caching, fresh arrays with equal content.
  const stateA = buildStateA();
  const callOne = buildChatThreadRenderableMessages(stateA);
  const callTwo = buildChatThreadRenderableMessages(stateA);
  assert.notEqual(callOne, callTwo);
  assert.deepEqual(callTwo, callOne);
});

test('chat thread projection handles empty states without throwing', () => {
  assert.deepEqual(buildChatThreadRenderableMessages(emptyChatThreadState('session-empty')), []);
  assert.deepEqual(buildChatThreadRenderableMessages(null), []);

  const turnOnly = emptyChatThreadState('session-turn-only');
  applyFrames(turnOnly, [
    turnUpsert(1, { turn_id: 'turn-1', user_round: 0, status: 'running', content: '' })
  ]);
  assert.deepEqual(buildChatThreadRenderableMessages(turnOnly), []);

  const userOnly = emptyChatThreadState('session-user-only');
  applyFrames(userOnly, [
    turnUpsert(1, { turn_id: 'turn-1', user_round: 0, status: 'running', content: 'plain question' })
  ]);
  const messages = buildChatThreadRenderableMessages(userOnly);
  assert.equal(messages.length, 1);
  assert.equal(messages[0].role, 'user');
  assert.equal(messages[0].content, 'plain question');
});

test('chat thread projection output materializes through the legacy render adapter', () => {
  const state = emptyChatThreadState('session-projection-materialize');
  applyFrames(state, [
    turnUpsert(1, { turn_id: 'turn-1', user_round: 0, status: 'running', content: 'q' }),
    itemUpsert(2, textItemData('turn-1', 0, { status: 'running', content: '' })),
    tailFrame('turn-1:text-0', 'content', 0, 'streaming answer'),
    itemUpsert(3, toolItemData('turn-1', 'call-a', {
      model_round: 0,
      event_type: 'tool_call',
      tool: 'lookup',
      tool_display_name: 'Lookup'
    }))
  ]);

  const messages = buildChatThreadRenderableMessages(state);
  for (const message of messages) {
    const materialized = materializeChatRuntimeMessage(message, { workflowActive: false });
    assert.ok(materialized, `message ${message.id} must materialize`);
    assert.equal(materialized.__runtime_projected, true);
    assert.equal(materialized.role, message.role);
    assert.equal(materialized.content, message.content);
  }

  const assistant = messages.find((message) => message.role === 'assistant');
  const materializedAssistant = materializeChatRuntimeMessage(assistant, { workflowActive: false });
  assert.ok(materializedAssistant);
  assert.equal(materializedAssistant.runtime_status, 'tooling');
  assert.equal(materializedAssistant.workflowStreaming, true);
  const workflowItems = materializedAssistant.workflowItems as Array<Record<string, unknown>>;
  assert.equal(workflowItems.length, 1);
  assert.equal(workflowItems[0].toolName, 'lookup');
  assert.equal(workflowItems[0].toolDisplayName, 'Lookup');
  assert.equal(workflowItems[0].status, 'loading');
});
