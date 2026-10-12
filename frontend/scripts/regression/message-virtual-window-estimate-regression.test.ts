import test from 'node:test';
import assert from 'node:assert/strict';

import {
  buildMessageVirtualWindow,
  buildVirtualOffsetPrefix,
  createMessageVirtualHeightEstimator,
  MESSAGE_VIRTUAL_SEGMENT_MULTIPLIERS,
  resolveMessageVirtualRowSegment,
  resolveVirtualOffsetTop,
  resolveVirtualOffsetTopFromPrefix
} from '../../src/views/messenger/messageVirtualWindow';

const BASE_HEIGHT = 118;

test('row keys resolve to their estimation segment', () => {
  assert.equal(resolveMessageVirtualRowSegment('turn:abc'), 'turn');
  assert.equal(resolveMessageVirtualRowSegment('turn:client:c1'), 'turn');
  assert.equal(resolveMessageVirtualRowSegment('turn:legacy:3'), 'turn');
  assert.equal(resolveMessageVirtualRowSegment('tturn:abc'), 'turn');
  assert.equal(resolveMessageVirtualRowSegment('conversation-greeting'), 'greeting');
  assert.equal(resolveMessageVirtualRowSegment('world:row-1'), 'world');
  assert.equal(resolveMessageVirtualRowSegment('message-7'), 'message');
  assert.equal(resolveMessageVirtualRowSegment(''), 'message');
  assert.equal(MESSAGE_VIRTUAL_SEGMENT_MULTIPLIERS.turn, 2);
});

test('unmeasured rows use their segment average instead of the flat turn x 2 estimate', () => {
  const estimator = createMessageVirtualHeightEstimator({ baseHeight: BASE_HEIGHT });
  // Without any sample a turn row still uses the default multiplier.
  assert.equal(estimator.resolve('turn:new'), BASE_HEIGHT * 2);
  assert.equal(estimator.resolve('conversation-greeting'), BASE_HEIGHT);

  // Measured turn rows shift the segment average to the real composite height.
  estimator.record('turn:a', 300);
  estimator.record('turn:b', 400);
  assert.equal(estimator.segmentEstimate('turn'), 350);
  assert.equal(estimator.resolve('turn:new'), 350);
  assert.equal(estimator.resolve('turn:a'), 300);

  // Re-measuring a key adjusts the running average without double counting.
  estimator.record('turn:a', 400);
  assert.equal(estimator.segmentEstimate('turn'), 400);
  assert.equal(estimator.size(), 2);

  // The greeting segment keeps its own estimate, untouched by turn samples.
  assert.equal(estimator.resolve('conversation-greeting'), BASE_HEIGHT);
});

test('estimator snapshot and hydrate round-trip the measured table', () => {
  const source = createMessageVirtualHeightEstimator({ baseHeight: BASE_HEIGHT });
  source.record('turn:1', 260);
  source.record('message-1', 90);

  const restored = createMessageVirtualHeightEstimator({ baseHeight: BASE_HEIGHT });
  restored.hydrate(source.snapshot());

  assert.equal(restored.measuredHeight('turn:1'), 260);
  assert.equal(restored.resolve('message-1'), 90);
  assert.equal(restored.segmentEstimate('turn'), 260);
  assert.equal(restored.size(), 2);
});

test('virtual offset prefix matches the running sum of row heights', () => {
  const keys = ['a', 'b', 'c', 'd'];
  const heights: Record<string, number> = { a: 80, b: 90, c: 110, d: 120 };
  const resolveHeight = (key: string): number => heights[key] || 0;

  const prefix = buildVirtualOffsetPrefix(keys, resolveHeight);
  assert.deepEqual(prefix, [0, 80, 170, 280, 400]);
  assert.equal(resolveVirtualOffsetTopFromPrefix(prefix, 3), 280);
  assert.equal(resolveVirtualOffsetTopFromPrefix(prefix, 0), 0);
  assert.equal(resolveVirtualOffsetTopFromPrefix(prefix, 99), 400);
  assert.equal(resolveVirtualOffsetTop(keys, 3, resolveHeight), 280);
  assert.equal(resolveVirtualOffsetTop(keys, 0, resolveHeight), 0);
  assert.equal(resolveVirtualOffsetTop([], 5, () => 100), 0);
});

test('resolveVirtualOffsetTop is constant-cost per lookup instead of a per-row scan', () => {
  // The legacy writing summed `resolveHeight` from 0 to `index`, so its cost
  // grew with the row count. The new writing builds the prefix once per key list
  // and every later lookup is an O(1) array read.
  const keys = Array.from({ length: 2000 }, (_value, index) => `turn:${index}`);
  let calls = 0;
  const resolveHeight = (key: string): number => {
    calls += 1;
    return 120 + (key.length % 3);
  };

  const warmed = resolveVirtualOffsetTop(keys, keys.length, resolveHeight);
  assert.equal(warmed > 0, true);
  // First lookup is the single O(keys.length) prefix build.
  assert.equal(calls, keys.length);

  calls = 0;
  const repeated = resolveVirtualOffsetTop(keys, keys.length, resolveHeight);
  assert.equal(repeated, warmed);
  assert.ok(calls <= 4, `expected a constant resolveHeight call count, got ${calls}`);

  // The cost neither grows with the requested index nor with the row count.
  for (const index of [10, 100, 1000, keys.length]) {
    calls = 0;
    resolveVirtualOffsetTop(keys, index, resolveHeight);
    assert.ok(calls <= 4, `index ${index} triggered ${calls} resolveHeight calls`);
  }

  // Counterfactual: the prefix read never calls resolveHeight at all.
  const prefix = buildVirtualOffsetPrefix(keys, resolveHeight);
  calls = 0;
  resolveVirtualOffsetTopFromPrefix(prefix, keys.length);
  assert.equal(calls, 0);
});

test('980 unmeasured rows keep the scrollbar estimate within 5 percent', () => {
  const rows: Array<{ key: string; trueHeight: number }> = [];
  rows.push({ key: 'conversation-greeting', trueHeight: 132 });
  for (let index = 0; index < 900; index += 1) {
    rows.push({ key: `turn:${index}`, trueHeight: 388 });
  }
  for (let index = 0; index < 79; index += 1) {
    rows.push({ key: `message-${index}`, trueHeight: 152 });
  }
  assert.equal(rows.length, 980);

  const estimator = createMessageVirtualHeightEstimator({ baseHeight: BASE_HEIGHT });
  // Measure a small sample per segment, the way the runtime reports real rows.
  estimator.record('conversation-greeting', 132);
  for (let index = 0; index < 900; index += 30) {
    estimator.record(`turn:${index}`, 388);
  }
  for (let index = 0; index < 79; index += 10) {
    estimator.record(`message-${index}`, 152);
  }
  assert.ok(estimator.size() < rows.length);

  const windowResult = buildMessageVirtualWindow({
    items: rows.map((row) => ({ key: row.key })),
    enabled: true,
    scrollTop: 0,
    viewportHeight: 900,
    overscan: 4,
    tailPinCount: 4,
    estimatedHeight: BASE_HEIGHT,
    resolveHeight: (key: string) => estimator.resolve(key)
  });

  let trueTotal = 0;
  let legacyTotal = 0;
  for (const row of rows) {
    trueTotal += row.trueHeight;
    // The old flat writing: a turn row is always base x 2, everything else base.
    legacyTotal += BASE_HEIGHT * (row.key.startsWith('turn:') ? 2 : 1);
  }

  const estimatedError = Math.abs(windowResult.totalHeight - trueTotal) / trueTotal;
  const legacyError = Math.abs(legacyTotal - trueTotal) / trueTotal;
  assert.ok(estimatedError <= 0.05, `segmented estimate error was ${estimatedError}`);
  // The legacy flat estimate must miss the bar, otherwise this guard is vacuous.
  assert.ok(legacyError > 0.05, `legacy estimate error was ${legacyError}`);
});
