import test from 'node:test';
import assert from 'node:assert/strict';

import {
  buildMessageVirtualWindow,
  resolveVirtualOffsetTop
} from '../../src/views/messenger/messageVirtualWindow';

test('message virtual window keeps tail items mounted while trimming history', () => {
  const items = Array.from({ length: 60 }, (_value, index) => ({
    key: `message-${index}`
  }));

  const result = buildMessageVirtualWindow({
    items,
    enabled: true,
    scrollTop: 0,
    viewportHeight: 600,
    overscan: 2,
    tailPinCount: 6,
    estimatedHeight: 100,
    resolveHeight: () => 100
  });

  assert.equal(result.enabled, true);
  assert.equal(result.visibleItems.length > 0, true);
  assert.equal(result.tailItems.length, 6);
  assert.equal(result.tailItems[0]?.key, 'message-54');
  assert.equal(result.startIndex, 0);
  assert.equal(result.endIndex > result.startIndex, true);
  assert.equal(result.tailStartIndex, 54);
  assert.equal(result.totalHeight, 6000);
  assert.equal(result.topPadding, 0);
  assert.equal(result.bottomPadding > 0, true);
});

test('message virtual window uses cached heights for non-zero scroll windows', () => {
  const items = Array.from({ length: 20 }, (_value, index) => ({
    key: `message-${index}`
  }));
  const heights = new Map<string, number>(
    items.map((item, index) => [item.key, index % 2 === 0 ? 80 : 120])
  );

  const result = buildMessageVirtualWindow({
    items,
    enabled: true,
    scrollTop: 620,
    viewportHeight: 300,
    overscan: 1,
    tailPinCount: 4,
    estimatedHeight: 100,
    resolveHeight: (key) => heights.get(key) || 100
  });

  assert.equal(result.enabled, true);
  assert.equal(result.tailStartIndex, 16);
  assert.equal(result.startIndex < result.endIndex, true);
  assert.equal(result.startIndex > 0, true);
  assert.equal(result.topPadding, resolveVirtualOffsetTop(items.map((item) => item.key), result.startIndex, (key) => heights.get(key) || 100));
  assert.equal(result.tailItems.length, 4);
});

test('message virtual window resolves virtual offset using cached heights', () => {
  const keys = ['a', 'b', 'c', 'd'];
  const heights = new Map<string, number>([
    ['a', 80],
    ['b', 90],
    ['c', 110],
    ['d', 120]
  ]);

  assert.equal(
    resolveVirtualOffsetTop(keys, 3, (key) => heights.get(key) || 0),
    280
  );
});
