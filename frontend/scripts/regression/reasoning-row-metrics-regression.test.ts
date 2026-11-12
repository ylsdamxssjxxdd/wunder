import test from 'node:test';
import assert from 'node:assert/strict';
import { resolveModelRoundMetrics } from '../../src/utils/modelRoundMetrics';

test('thinking row reads the round context occupancy and its own thinking span', () => {
  const metrics = resolveModelRoundMetrics({
    reasoning: '思考',
    usage: { input_tokens: 175889, output_tokens: 83, reasoning_tokens: 145 },
    stream_timing: { decode_ms: 1008, content_decode_ms: null, reasoning_ms: 4200 }
  });
  assert.deepEqual(metrics, { tokens: 175889, durationMs: 4200 });
});

test('thread view payload wrapper resolves the same two columns', () => {
  const metrics = resolveModelRoundMetrics({
    item_id: 'turn:text-1',
    payload: { request_context_tokens: 2400, stream_timing: { reasoning_ms: 900 } }
  });
  assert.deepEqual(metrics, { tokens: 2400, durationMs: 900 });
});

test('a legacy round without a thinking span keeps the token column only', () => {
  const metrics = resolveModelRoundMetrics({
    usage: { input_tokens: 1000 },
    stream_timing: { decode_ms: 5000 }
  });
  assert.deepEqual(metrics, { tokens: 1000, durationMs: null });
});

test('no usage and no span renders nothing instead of an empty pair', () => {
  assert.equal(resolveModelRoundMetrics({ reasoning: '思考' }), null);
  assert.equal(resolveModelRoundMetrics(null), null);
  assert.equal(resolveModelRoundMetrics({ usage: { input_tokens: 0 } }), null);
});
