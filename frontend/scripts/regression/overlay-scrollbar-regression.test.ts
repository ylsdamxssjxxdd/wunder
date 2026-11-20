import test from 'node:test';
import assert from 'node:assert/strict';

import {
  overlayScrollbarDragOffset,
  overlayScrollbarMetrics,
} from '../../src/utils/overlayScrollbar';

// 覆盖式细滚动条的几何是纯换算：滑块长度反映可见比例、偏移反映滚动进度，
// 一旦算错就会出现「滑块铺满却还能滚」「滚到底滑块不贴底」这类手感问题。

test('内容不溢出时报告不可滚动，滑块铺满轨道', () => {
  const metrics = overlayScrollbarMetrics({ viewport: 420, content: 420, offset: 0, track: 400 });
  assert.deepEqual(metrics, { scrollable: false, thumbSize: 400, thumbOffset: 0 });
});

test('亚像素级溢出不算滚动', () => {
  const metrics = overlayScrollbarMetrics({ viewport: 400, content: 400.4, offset: 0, track: 400 });
  assert.equal(metrics.scrollable, false);
});

test('滑块长度按可见比例收缩，偏移按滚动进度贴住首尾', () => {
  const input = { viewport: 200, content: 800, offset: 0, track: 400 };
  const atTop = overlayScrollbarMetrics(input);
  assert.equal(atTop.scrollable, true);
  assert.equal(atTop.thumbSize, 100);
  assert.equal(atTop.thumbOffset, 0);

  const atBottom = overlayScrollbarMetrics({ ...input, offset: 600 });
  assert.equal(atBottom.thumbOffset, 300);

  const middle = overlayScrollbarMetrics({ ...input, offset: 300 });
  assert.equal(middle.thumbOffset, 150);
});

test('超长内容仍保留可抓取的最小滑块长度', () => {
  const metrics = overlayScrollbarMetrics({ viewport: 200, content: 200_000, offset: 0, track: 400 });
  assert.equal(metrics.scrollable, true);
  assert.equal(metrics.thumbSize, 28);
  // 滑块变短后行程变长，滚到底仍要贴住轨道末端。
  const bottom = overlayScrollbarMetrics({ viewport: 200, content: 200_000, offset: 199_800, track: 400 });
  assert.equal(bottom.thumbOffset, 372);
});

test('最小滑块长度不超过轨道本身', () => {
  const metrics = overlayScrollbarMetrics({ viewport: 10, content: 10_000, offset: 0, track: 12 });
  assert.equal(metrics.thumbSize, 12);
  assert.equal(metrics.thumbOffset, 0);
});

test('偏移越界与非法尺寸都被夹紧，不会把滑块甩出轨道', () => {
  const overscroll = overlayScrollbarMetrics({ viewport: 200, content: 800, offset: 9999, track: 400 });
  assert.equal(overscroll.thumbOffset, 300);

  const negative = overlayScrollbarMetrics({ viewport: 200, content: 800, offset: -50, track: 400 });
  assert.equal(negative.thumbOffset, 0);

  assert.deepEqual(overlayScrollbarMetrics({ viewport: 200, content: 800, offset: 0, track: 0 }), {
    scrollable: false,
    thumbSize: 0,
    thumbOffset: 0,
  });
  const hidden = overlayScrollbarMetrics({ viewport: 0, content: 800, offset: 0, track: 400 });
  assert.equal(hidden.scrollable, false);
});

test('拖动位移按可移动轨道换算成滚动偏移', () => {
  // 内容 800 / 视口 200 / 轨道 400 / 滑块 100：滑块走完 300px 对应内容滚完 600px。
  assert.equal(
    overlayScrollbarDragOffset({ delta: 300, track: 400, thumbSize: 100, viewport: 200, content: 800 }),
    600,
  );
  assert.equal(
    overlayScrollbarDragOffset({ delta: 150, track: 400, thumbSize: 100, viewport: 200, content: 800 }),
    300,
  );
  // 不可滚动或轨道被滑块占满时拖动不产生位移。
  assert.equal(
    overlayScrollbarDragOffset({ delta: 40, track: 400, thumbSize: 400, viewport: 200, content: 200 }),
    0,
  );
});
