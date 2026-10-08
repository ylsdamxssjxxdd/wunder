import test from 'node:test';
import assert from 'node:assert/strict';
import { createChatRuntimeProjection, applyChatRuntimeEvent } from '../../src/realtime/chat/chatRuntimeReducer';
import { buildCanonicalChatRuntimeEvents } from '../../src/realtime/chat/chatCanonicalEvents';
import { selectVisibleMessageProjections } from '../../src/realtime/chat/chatRuntimeSelectors';
import { buildWorkflowToolRuns } from '../../src/components/chat/toolWorkflowRunModel';
import { resolveCollapsedWorkflowEntryMetadata } from '../../src/components/chat/toolWorkflowCollapsedMetadata';
import { resolveComposerContextDisplay } from '../../src/components/chat/composerContextDisplay';
import { resolveComposerContextUsageSource } from '../../src/components/chat/composerContextUsage';

const fixture = () => {
  const projection = createChatRuntimeProjection();
  let seq = 0;
  const emit = (type: string, data: Record<string, unknown>, round = 1, time = 0) => {
    const events = buildCanonicalChatRuntimeEvents({ sessionId: 'session_1', eventType: type,
      eventId: ++seq, payload: {timestamp: new Date(1800000000000 + time).toISOString(),
        data: { user_round: 1, model_round: round, ...data }} });
    events.forEach(event => applyChatRuntimeEvent(projection, event));
    return events;
  };
  const messages = () => selectVisibleMessageProjections(projection, 'session_1');
  const rows = () => messages().flatMap(message => buildWorkflowToolRuns(message.workflowItems || []));
  return { projection, emit, rows, messages };
};

test('parallel tools keep request context and runtime latency despite conflicting result fields and later rounds', () => {
  const { emit, rows } = fixture();
  emit('context_usage', {context_tokens: 40, max_context: 1000});
  for (const id of ['call_1', 'call_2']) {
    emit('tool_call', {tool: 'read_file', tool_call_id: id, request_context_tokens: 120,
      request_usage: {total_tokens: 170}, args: {count: 999, duration_ms: 9999}});
  }
  emit('tool_result', {tool: 'read_file', tool_call_id: 'call_2', ok: false,
    meta: {duration_ms: 0}, data: {duration_ms: 9000, context_tokens: 999, count: 987}});
  emit('tool_result', {tool: 'read_file', tool_call_id: 'call_1', ok: true,
    meta: {duration_ms: 1250}, data: {output: 'x'.repeat(100_000), duration_ms: 7}}, 1, 3000);
  emit('context_usage', {context_tokens: 240}, 2);
  emit('tool_call', {tool: 'read_file', tool_call_id: 'call_3', request_context_tokens: 240}, 2);
  assert.deepEqual(rows().map(row => ({...resolveCollapsedWorkflowEntryMetadata(row),
    status: row.resultItem?.status})).map(({contextTokensLabel, durationLabel, status}) =>
      ({contextTokensLabel, durationLabel, status})), [
    {contextTokensLabel:'120 token',durationLabel:'1.3s',status:'completed'},
    {contextTokensLabel:'120 token',durationLabel:'0ms',status:'failed'},
    {contextTokensLabel:'240 token',durationLabel:'',status:undefined}
  ]);
});

test('missing duration uses server event interval and replay does not change it', () => {
  const { emit, rows, projection } = fixture();
  emit('tool_call', {tool: 'read_file', tool_call_id: 'call_1'}, 1, 100);
  const events = emit('tool_result', {tool:'read_file',tool_call_id:'call_1',ok:true,data:{}}, 1, 450);
  events.forEach(event => applyChatRuntimeEvent(projection, event));
  assert.equal(resolveCollapsedWorkflowEntryMetadata(rows()[0]).durationLabel, '350ms');
});

test('cancellation settles a pending tool with an event interval', () => {
  const { emit, rows } = fixture();
  emit('tool_call', {tool:'read_file',tool_call_id:'call_1'}, 1, 100);
  emit('turn_terminal', {status:'cancelled'}, 1, 1100);
  assert.equal(resolveCollapsedWorkflowEntryMetadata(rows()[0]).durationLabel, '1.0s');
});

test('zero context survives a partial snapshot and unknown request context never inherits stale occupancy', () => {
  const { emit, rows } = fixture();
  emit('context_usage', {context_tokens: 50, max_context: 1000});
  emit('context_usage', {context_tokens: 0});
  emit('context_usage', {max_context: 800});
  emit('tool_call', {tool:'read_file',tool_call_id:'call_1'});
  emit('tool_call', {tool:'read_file',tool_call_id:'call_2',request_context_tokens:null});
  assert.deepEqual(rows().map(row => resolveCollapsedWorkflowEntryMetadata(row).contextTokensLabel),
    ['0 token', '']);
  assert.equal(rows()[0].callItem?.['context_max_tokens'], 800);
});

test('provider usage contributes input occupancy, never output or reasoning', () => {
  const {emit, messages} = fixture();
  emit('llm_request', {});
  emit('token_usage', {input_tokens:120,output_tokens:10,reasoning_tokens:40,total_tokens:170});
  assert.equal(messages().find(message => message.role === 'assistant')?.display?.stats?.contextTokens, 120);
});

test('composer holds missing fields, accepts decreases and zero, and isolates sessions', () => {
  let state = resolveComposerContextDisplay(undefined,
    {scope:'session_1',assistant:'message_1',observed:true,used:500,total:1000});
  state = resolveComposerContextDisplay(state, {...state,observed:false,used:900,total:null});
  assert.deepEqual([state.used,state.total], [500,1000]);
  state = resolveComposerContextDisplay(state, {...state,assistant:'message_2',observed:false,used:900});
  assert.equal(state.used, 500);
  state = resolveComposerContextDisplay(state, {...state,observed:true,used:0,total:800});
  assert.deepEqual([state.used,state.total], [0,800]);
  state = resolveComposerContextDisplay(state, {...state,scope:'session_2',observed:false,used:null,total:null});
  assert.deepEqual([state.used,state.total], [null,null]);
});

test('composer resumes observations after compaction instead of remaining pinned to zero', () => {
  const message = {role:'assistant',content:'',manual_compaction_marker:true,stream_incomplete:true,
    stats:{contextTokens:200,contextSnapshotSeq:12},workflowItems:[
      {eventType:'compaction',status:'completed',updatedSeq:10,
        detail:JSON.stringify({status:'done',final_context_tokens:0})}
    ]};
  const result = resolveComposerContextUsageSource([message], {context_tokens:0,context_max_tokens:1000}, true);
  assert.equal(result.contextTokens, 200);
});
