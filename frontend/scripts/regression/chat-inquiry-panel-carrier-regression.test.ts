import test from 'node:test';
import assert from 'node:assert/strict';

import { emptyChatThreadState, type ChatThreadState, type ThreadChangeFrame } from '../../src/realtime/chat/chatThreadTypes';
import { applyChatThreadFrame, applyChatThreadSnapshot } from '../../src/realtime/chat/chatThreadState';
import { buildChatThreadRenderableMessages } from '../../src/realtime/chat/chatThreadProjection';
import { materializeChatRuntimeMessage } from '../../src/realtime/chat/chatRuntimeRenderAdapter';
import {
  buildCanonicalChatRuntimeEvents
} from '../../src/realtime/chat/chatCanonicalEvents';
import {
  createChatRuntimeProjection,
  applyChatRuntimeEvent
} from '../../src/realtime/chat/chatRuntimeReducer';
import { selectVisibleMessageProjections } from '../../src/realtime/chat/chatRuntimeSelectors';

const turnUpsert = (seq: number, data: Record<string, unknown>): ThreadChangeFrame => ({
  event: 'thread_change', seq, change_type: 'turn_upsert',
  turn_id: String(data.turn_id), data
});

const itemUpsert = (seq: number, data: Record<string, unknown>): ThreadChangeFrame => ({
  event: 'thread_change', seq, change_type: 'item_upsert',
  item_id: String(data.item_id), turn_id: String(data.turn_id), data
});

const carrierQuestions = [
  {
    question: 'Choose a route',
    multiple: false,
    options: [
      { label: 'Fast', description: 'Use the fast route', recommended: true },
      { label: 'Careful', description: 'Run extra checks' }
    ]
  }
];

const carrier = {
  action: 'question_panel',
  state: 'awaiting_input',
  data: { questions: carrierQuestions }
};

const materializeAssistant = (state: ChatThreadState) => {
  const projected = buildChatThreadRenderableMessages(state).find((message) => message.role === 'assistant');
  return materializeChatRuntimeMessage(projected)! as Record<string, any>;
};

test('durable question-panel carrier promotes an independent composer panel', () => {
  const state = emptyChatThreadState('inquiry-carrier-session');
  const turnId = 'inquiry-carrier-turn';
  const textId = `${turnId}:text-1`;
  const toolId = `${turnId}:tool-question`;
  applyFrames(state, [
    turnUpsert(1, { turn_id: turnId, user_round: 1, status: 'waiting_input', content: 'Choose a route.' }),
    itemUpsert(2, { item_id: `${turnId}:user`, turn_id: turnId, kind: 'user_message', role: 'user',
      visibility: 'user', status: 'completed', revision: 1, content: 'Choose a route.' }),
    itemUpsert(3, { item_id: textId, turn_id: turnId, kind: 'assistant_message', role: 'assistant',
      visibility: 'user', status: 'completed', revision: 1, content: 'I need one choice before continuing.' }),
    // This is the compact tool-result shape emitted by the question-panel tool.
    // The panel must also be projected onto the assistant message so the
    // composer can render it above the input area after a reload.
    itemUpsert(4, { item_id: toolId, turn_id: turnId, kind: 'tool_result', tool: 'question_panel',
      event_type: 'tool_result', visibility: 'user', status: 'completed', revision: 1,
      payload: { data: carrier } })
  ]);

  const assistant = materializeAssistant(state);
  assert.equal(assistant.questionPanel?.status, 'pending');
  assert.equal(assistant.questionPanel?.keepOpen, true);
  assert.equal(assistant.questionPanel?.questions?.[0]?.question, 'Choose a route');
  assert.equal(assistant.questionPanel?.questions?.[0]?.options?.[0]?.label, 'Fast');
  assert.equal(assistant.questionPanel?.questions?.[0]?.options?.[0]?.recommended, true);
  // The carrier is an interaction surface owned by the composer. It must not
  // be duplicated as a generic tool card in the workflow list.
  assert.equal(assistant.workflowItems?.some((item: any) => item.eventType === 'tool_result'), false);
});

test('carrier panel survives atomic snapshot replacement and settles after a user answer', () => {
  const state = emptyChatThreadState('inquiry-carrier-reload');
  const turnId = 'inquiry-reload-turn';
  const textId = `${turnId}:text-1`;
  const toolId = `${turnId}:tool-question`;
  const turn = { turn_id: turnId, user_round: 1, status: 'waiting_input', content: 'Choose a route.' };
  const text = { item_id: textId, turn_id: turnId, kind: 'assistant_message', role: 'assistant',
    visibility: 'user', status: 'completed', revision: 1, content: 'Waiting for your selection.' };
  const tool = { item_id: toolId, turn_id: turnId, kind: 'tool_call', tool: 'question_panel',
    event_type: 'tool_result', visibility: 'user', status: 'completed', revision: 1,
    payload: { data: carrier } };
  applyChatThreadSnapshot(state, { cursor: 4, turns: [turn], items: [
    { item_id: `${turnId}:user`, turn_id: turnId, kind: 'user_message', role: 'user',
      visibility: 'user', status: 'completed', revision: 1, content: turn.content },
    text, tool
  ] });
  assert.equal(materializeAssistant(state).questionPanel?.status, 'pending');

  // A persisted answered panel is no longer eligible for the composer, while
  // the historical questions remain available for the message record.
  const answeredCarrier = { ...carrier, state: 'answered', data: {
    ...carrier.data,
    answers: [{ questionIndex: 0, labels: ['Fast'], other: '', noPreference: false }]
  } };
  applyChatThreadSnapshot(state, { cursor: 5, turns: [{ ...turn, status: 'completed' }], items: [
    { item_id: `${turnId}:user`, turn_id: turnId, kind: 'user_message', role: 'user',
      visibility: 'user', status: 'completed', revision: 1, content: turn.content },
    { ...text, revision: 2 },
    { ...tool, revision: 2, payload: { data: answeredCarrier } }
  ] });
  const settled = materializeAssistant(state);
  assert.equal(settled.questionPanel?.status, 'answered');
  assert.equal(settled.questionPanel?.questions?.[0]?.options?.[0]?.label, 'Fast');
});

test('runtime question-panel event unwraps action/state/data carrier', () => {
  const projection = createChatRuntimeProjection();
  buildCanonicalChatRuntimeEvents({
    sessionId: 'runtime-inquiry-carrier',
    eventType: 'question_panel',
    eventId: 1,
    requestId: 'runtime-inquiry-request',
    payload: { data: carrier }
  }).forEach((event) => applyChatRuntimeEvent(projection, event));
  const assistant = selectVisibleMessageProjections(projection, 'runtime-inquiry-carrier')
    .find((message) => message.role === 'assistant');
  assert.equal(assistant?.display?.questionPanel &&
    (assistant.display.questionPanel as any).status, 'pending');
  assert.equal((assistant?.display?.questionPanel as any)?.questions?.[0]?.question, 'Choose a route');
});

const applyFrames = (state: ChatThreadState, frames: ThreadChangeFrame[]): void => {
  frames.forEach((frame) => applyChatThreadFrame(state, frame, Date.now()));
};
