import test from 'node:test';
import assert from 'node:assert/strict';
import { buildCanonicalChatRuntimeEvents } from '../../src/realtime/chat/chatCanonicalEvents';
import { buildCanonicalSessionEventsSnapshot } from '../../src/realtime/chat/chatRuntimeBridge';
import { applyChatRuntimeEvent, createChatRuntimeProjection } from '../../src/realtime/chat/chatRuntimeReducer';
import { materializeChatRuntimeMessages } from '../../src/realtime/chat/chatRuntimeRenderAdapter';
import { selectSessionBusy } from '../../src/realtime/chat/chatRuntimeSelectors';
import { buildAssistantMessageStatsEntries } from '../../src/utils/messageStats';

const t = (key: string) => key;
const createHarness = () => {
  const projection = createChatRuntimeProjection();
  let seq = 0;
  const records: Record<string, unknown>[] = [];
  const apply = (type: string, data: Record<string, unknown> = {}) => {
    const payload = {
      session_id: 'session-1', timestamp: new Date().toISOString(),
      data: { user_round: 1, model_round: 1, ...data }
    };
    const options = { sessionId: 'session-1', eventType: type, eventId: ++seq, payload };
    const events = buildCanonicalChatRuntimeEvents(options);
    records.push({ event: type, event_id: seq, data: payload });
    events.forEach(event => applyChatRuntimeEvent(projection, event));
    return events;
  };
  const message = () => materializeChatRuntimeMessages(projection, 'session-1').at(-1)!;
  const status = () => buildAssistantMessageStatsEntries(message(), t, undefined, Date.now())[0];
  apply('llm_request');
  apply('llm_output_delta', { reasoning_delta: 'Working' });
  return { projection, records, apply, message, status };
};

const invalidCall = { attempt: 1, max_attempts: 6, retry_reason: 'invalid_tool_call_arguments', will_retry: true, delay_s: 1.2 };

test('invalid tool call is immediately visible after reasoning and survives replay', () => {
  const h = createHarness();
  const events = h.apply('bad_tool_call_retry', invalidCall);
  assert.equal(events.length, 1);
  assert.ok(h.status().value.includes('toolCallRetrying'));
  assert.equal(h.status().tone, 'warning');
  assert.equal(h.message().retry_reason, 'invalid_tool_call_arguments');
  assert.equal(selectSessionBusy(h.projection, 'session-1'), true);
  const count = (h.message().workflowItems as unknown[]).length;
  events.forEach(event => applyChatRuntimeEvent(h.projection, event));
  assert.equal((h.message().workflowItems as unknown[]).length, count);

  const replay = createChatRuntimeProjection();
  buildCanonicalSessionEventsSnapshot({ sessionId: 'session-1', payload: { events: h.records } })
    .forEach(event => applyChatRuntimeEvent(replay, event));
  const restored = materializeChatRuntimeMessages(replay, 'session-1').at(-1)!;
  assert.equal(restored.retry_reason, 'invalid_tool_call_arguments');
  assert.ok(buildAssistantMessageStatsEntries(restored, t)[0].value.includes('toolCallRetrying'));
});

for (const output of ['llm_output_delta', 'llm_output', 'tool_call']) {
  test(`valid ${output} clears recovery without leaving a loading retry item`, () => {
    const h = createHarness();
    h.apply('bad_tool_call_retry', invalidCall);
    h.apply(output, output === 'tool_call'
      ? { tool: 'read_file', tool_call_id: 'call-1' }
      : { delta: 'Text', content: 'Text', tool_calls: [{ id: 'call-1' }] });
    assert.equal(h.message().retry_state, undefined);
    assert.ok(!h.status().value.includes('Retrying'));
    const item = (h.message().workflowItems as Record<string, unknown>[])
      .find(item => item.eventType === 'bad_tool_call_retry');
    assert.equal(item?.status, 'completed');
  });
}

for (const terminal of ['failed', 'cancelled', 'completed']) {
  test(`${terminal} terminal clears retry state and stops loading even after idle and replay`, () => {
    const h = createHarness();
    h.apply('bad_tool_call_retry', { ...invalidCall, attempt: 6, will_retry: false, delay_s: 0 });
    assert.ok(h.status().value.includes('retryExhausted'));
    h.apply('turn_terminal', { status: terminal });
    h.apply('thread_status', { status: 'idle' });
    h.apply('thread_closed', { status: 'not_loaded' });
    assert.equal(h.message().retry_state, undefined);
    assert.equal(h.message().workflowStreaming, false);
    assert.equal(selectSessionBusy(h.projection, 'session-1'), false);
    assert.equal(h.status().live, false);
    if (terminal === 'failed') assert.equal(h.status().value, 'messenger.messageStatus.error');
    h.apply('bad_tool_call_retry', invalidCall);
    assert.equal(h.message().retry_state, undefined);
    assert.equal(selectSessionBusy(h.projection, 'session-1'), false);
    const replay = createChatRuntimeProjection();
    buildCanonicalSessionEventsSnapshot({ sessionId: 'session-1', payload: { events: h.records } })
      .forEach(event => applyChatRuntimeEvent(replay, event));
    const restored = materializeChatRuntimeMessages(replay, 'session-1').at(-1)!;
    assert.equal(restored.retry_state, undefined);
    assert.equal(restored.workflowStreaming, false);
  });
}

for (const stage of ['invalid_tool_call_reroute', 'empty_final_answer_reroute']) {
  test(`${stage} stays visible during the recovery model request`, () => {
    const h = createHarness();
    h.apply('progress', { stage, attempt: 1, max_attempts: 3 });
    h.apply('progress', { stage: 'llm_call', model_round: 2 });
    h.apply('llm_request', { model_round: 2 });
    assert.equal(h.message().retry_reason, stage);
    assert.ok(h.status().value.includes(stage.startsWith('invalid') ? 'toolCallRetrying' : 'emptyOutputRetrying'));
    h.apply('llm_output_delta', { delta: 'Recovered', model_round: 2 });
    assert.equal(h.message().retry_state, undefined);
  });
}

test('retry countdown switches to elapsed waiting instead of repeating the initial delay', () => {
  const h = createHarness();
  h.apply('bad_tool_call_retry', invalidCall);
  const message = h.message();
  const now = Number(message.retry_started_at_ms) + 5_000;
  const status = buildAssistantMessageStatsEntries(message, t, undefined, now)[0];
  assert.ok(status.value.includes('retryElapsedCompact'));
  assert.ok(!status.value.includes('retryDelayCompact'));
});
