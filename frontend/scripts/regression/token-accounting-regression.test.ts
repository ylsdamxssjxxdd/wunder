import test from 'node:test';
import assert from 'node:assert/strict';
import { normalizeTokenUsage } from '../../src/utils/tokenUsage';
import { createChatRuntimeProjection, applyChatRuntimeEvent } from '../../src/realtime/chat/chatRuntimeReducer';
import { buildCanonicalChatRuntimeEvents } from '../../src/realtime/chat/chatCanonicalEvents';
import { selectVisibleMessageProjections } from '../../src/realtime/chat/chatRuntimeSelectors';
import { sumConversationConsumedTokens } from '../../src/utils/messageStats';
import { resolveComposerContextUsageSource } from '../../src/components/chat/composerContextUsage';

test('reasoning-only output preserves zero and unknown reasoning stays unknown', () => {
  assert.deepEqual(normalizeTokenUsage({input_tokens:100, output_tokens:0, total_tokens:140, reasoning_tokens:40}),
    {input:100, output:0, total:140, reasoning:40});
  assert.deepEqual(normalizeTokenUsage({input_tokens:100, output_tokens:40, total_tokens:140}),
    {input:100, output:40, total:140});
  assert.deepEqual(normalizeTokenUsage({promptTokens:10, completionTokens:20}), {input:10, output:20, total:30});
});

test('cumulative model accounting survives replay without quota or occupancy contamination', () => {
  const projection = createChatRuntimeProjection();
  let id = 0;
  const emit = (eventType: string, data: Record<string, unknown>, eventId = ++id) => {
    const events = buildCanonicalChatRuntimeEvents({ sessionId:'session_1', requestId:'request_1',
      userTurnId:'turn_1', modelTurnId:'model_1', assistantMessageId:'message_1', eventId,
      eventType, payload:{data:{user_round:1, model_round:1, ...data}} });
    events.forEach(event => applyChatRuntimeEvent(projection, event));
  };
  emit('llm_request', {});
  emit('context_usage', {context_occupancy_tokens:400, max_context:1000});
  const usage = {input_tokens:100, output_tokens:0, reasoning_tokens:40, total_tokens:140};
  emit('model_usage', {usage, round_usage:usage, request_consumed_tokens:140});
  emit('model_usage', {usage, round_usage:usage, request_consumed_tokens:140}, id);
  emit('quota_usage', {consumed:140, used:9000, remaining:1000});
  emit('token_usage', {input_tokens:1000, output_tokens:40, total_tokens:1040, estimated:true});
  emit('context_usage', {context_occupancy_tokens:0});
  const assistant = selectVisibleMessageProjections(projection, 'session_1').find(message => message.role === 'assistant');
  assert.ok(assistant);
  assert.deepEqual(assistant.display?.stats?.roundUsage, {input:100, output:0, reasoning:40, total:140});
  assert.equal(assistant.display?.stats?.quotaConsumed, 140);
  assert.equal(assistant.display?.stats?.contextTokens, 0);
});

test('history sums one cumulative snapshot per user turn in a single pass', () => {
  assert.equal(sumConversationConsumedTokens([
    {role:'user'}, {role:'assistant',stats:{quotaConsumed:100}},
    {role:'assistant',stats:{quotaConsumed:240}}, {role:'assistant',stats:{quotaConsumed:240}},
    {role:'user'}, {role:'assistant',stats:{quotaConsumed:80}}
  ]), 320);
});

test('composer accepts zero occupancy and reduced model capacity', () => {
  const source = resolveComposerContextUsageSource([{role:'assistant', stream_incomplete:true,
    stats:{context_occupancy_tokens:0, contextTotalTokens:1000}}],
    {context_occupancy_tokens:300, context_max_tokens:2000}, true);
  assert.equal(source.contextTokens, 0);
  assert.equal(source.contextTotalTokens, 1000);
});
