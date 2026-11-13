// AI生成 · 时间线折叠行右侧的模型轮次列：R1/R2 必须来自条目所属的那次模型请求。
import test from 'node:test';
import assert from 'node:assert/strict';

import type {
  ChatThreadFrame,
  ChatThreadState,
  ThreadChangeFrame
} from '../../src/realtime/chat/chatThreadTypes';
import { emptyChatThreadState } from '../../src/realtime/chat/chatThreadTypes';
import { applyChatThreadFrame } from '../../src/realtime/chat/chatThreadState';
import { buildChatThreadRenderableMessages } from '../../src/realtime/chat/chatThreadProjection';
import { createChatRuntimeProjection, applyChatRuntimeEvent } from '../../src/realtime/chat/chatRuntimeReducer';
import { buildCanonicalChatRuntimeEvents } from '../../src/realtime/chat/chatCanonicalEvents';
import { selectVisibleMessageProjections } from '../../src/realtime/chat/chatRuntimeSelectors';
import { buildWorkflowToolRuns, type RawToolRun } from '../../src/components/chat/toolWorkflowRunModel';
import { buildTimelineToolEntry } from '../../src/components/chat/toolTimelineModel';

const t = (key: string): string => key;

const itemUpsert = (seq: number, data: Record<string, unknown>): ThreadChangeFrame => ({
  event: 'thread_change',
  seq,
  change_type: 'item_upsert',
  data
});

const turnUpsert = (seq: number, data: Record<string, unknown>): ThreadChangeFrame => ({
  event: 'thread_change',
  seq,
  change_type: 'turn_upsert',
  data
});

const applyFrames = (state: ChatThreadState, frames: ChatThreadFrame[]): void => {
  frames.forEach((frame, index) => applyChatThreadFrame(state, frame, index));
};

const textItemData = (turnId: string, round: number, extra: Record<string, unknown> = {}): Record<string, unknown> => ({
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

const toolItemData = (turnId: string, callId: string, extra: Record<string, unknown> = {}): Record<string, unknown> => ({
  item_id: `${turnId}:tool-${callId}`,
  turn_id: turnId,
  kind: 'tool_call',
  visibility: 'user',
  status: 'running',
  revision: 1,
  tool_call_id: callId,
  ...extra
});

const roundOfEntries = (message: { workflowItems?: unknown } | undefined): Array<number | null> =>
  buildWorkflowToolRuns((message?.workflowItems ?? []) as never[])
    .map((run) => buildTimelineToolEntry(run, t).modelRound);

test('durable tool rows and thinking rows keep their own model round', () => {
  const state = emptyChatThreadState('fixture-round-session');
  applyFrames(state, [
    turnUpsert(1, { turn_id: 'turn-1', user_round: 1, status: 'running', content: 'fixture question' }),
    itemUpsert(2, textItemData('turn-1', 1, { reasoning: 'first thought', content: '' })),
    itemUpsert(3, toolItemData('turn-1', 'call-a', {
      model_round: 1, event_type: 'tool_call', tool: 'read_file', tool_display_name: '读取文件'
    })),
    itemUpsert(4, textItemData('turn-1', 2, { reasoning: 'second thought', content: 'fixture answer' })),
    itemUpsert(5, toolItemData('turn-1', 'call-b', {
      model_round: 2, event_type: 'tool_result', status: 'completed', tool: 'read_file'
    }))
  ]);
  const assistant = buildChatThreadRenderableMessages(state).find((row) => row.role === 'assistant');
  assert.ok(assistant);
  // 工具行：R1 的调用与 R2 的调用各自带自己的轮次。
  assert.deepEqual(roundOfEntries(assistant), [1, 2]);
  // 思考行：同样按所属轮次下发，渲染层才能和工具行同列对齐。
  const reasoningRounds = (assistant.timeline ?? [])
    .filter((block) => block.kind === 'activity')
    .flatMap((block) => block.rows)
    .filter((row) => row.type === 'reasoning')
    .map((row) => row.round);
  assert.deepEqual(reasoningRounds, [1, 2]);
});

test('live tool events carry the model round into the folded entry', () => {
  const projection = createChatRuntimeProjection();
  let seq = 0;
  const emit = (type: string, data: Record<string, unknown>, round = 1, time = 0): void => {
    const events = buildCanonicalChatRuntimeEvents({
      sessionId: 'session_1',
      eventType: type,
      eventId: ++seq,
      payload: {
        timestamp: new Date(1800000000000 + time).toISOString(),
        data: { user_round: 1, model_round: round, ...data }
      }
    });
    events.forEach((event) => applyChatRuntimeEvent(projection, event));
  };
  emit('tool_call', { tool: 'read_file', tool_call_id: 'call_1' }, 1, 100);
  emit('tool_result', { tool: 'read_file', tool_call_id: 'call_1', ok: true, data: {} }, 1, 900);
  emit('tool_call', { tool: 'read_file', tool_call_id: 'call_2' }, 2, 1000);
  const assistant = selectVisibleMessageProjections(projection, 'session_1')
    .find((message) => message.role === 'assistant');
  assert.ok(assistant);
  assert.deepEqual(roundOfEntries(assistant), [1, 2]);
});

test('a legacy row without a model round prints no round column', () => {
  const run: RawToolRun = {
    key: 'legacy-run',
    toolName: 'read_file',
    toolDisplayName: '读取文件',
    toolRuntimeName: 'read_file',
    toolFunctionName: '',
    callItem: { detail: '{}' },
    outputItem: null,
    resultItem: null
  };
  assert.equal(buildTimelineToolEntry(run, t).modelRound, null);
});
