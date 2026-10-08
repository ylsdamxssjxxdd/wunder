import test from 'node:test';
import assert from 'node:assert/strict';

import { createToolWorkflowRenderBatcher } from '../../src/components/chat/toolWorkflowRenderBatcher';

test('tool workflow render batcher coalesces rapid stream updates', () => {
  let currentTime = 0;
  const scheduled: Array<() => void> = [];
  const originalWindow = (globalThis as Record<string, unknown>).window;
  try {
    (globalThis as Record<string, unknown>).window = {
      setTimeout: (callback: () => void) => {
        scheduled.push(callback);
        return scheduled.length;
      },
      clearTimeout: () => {}
    };
    let flushCount = 0;
    const batcher = createToolWorkflowRenderBatcher(() => {
      flushCount += 1;
    }, {
      intervalMs: 96,
      now: () => currentTime
    });

    batcher.request();
    batcher.request();
    batcher.request();
    assert.equal(scheduled.length, 1);
    scheduled.shift()?.();
    assert.equal(flushCount, 1);

    currentTime = 10;
    batcher.request();
    batcher.request();
    assert.equal(scheduled.length, 1);
    scheduled.shift()?.();
    assert.equal(flushCount, 2);
    batcher.dispose();
  } finally {
    if (originalWindow === undefined) {
      delete (globalThis as Record<string, unknown>).window;
    } else {
      (globalThis as Record<string, unknown>).window = originalWindow;
    }
  }
});

test('tool workflow render batcher flushes structural updates immediately', () => {
  const scheduled: Array<() => void> = [];
  const originalWindow = (globalThis as Record<string, unknown>).window;
  try {
    (globalThis as Record<string, unknown>).window = {
      setTimeout: (callback: () => void) => {
        scheduled.push(callback);
        return scheduled.length;
      },
      clearTimeout: () => {}
    };
    let flushCount = 0;
    const batcher = createToolWorkflowRenderBatcher(() => {
      flushCount += 1;
    });
    batcher.request();
    batcher.request(true);
    assert.equal(flushCount, 1);
    batcher.dispose();
  } finally {
    if (originalWindow === undefined) {
      delete (globalThis as Record<string, unknown>).window;
    } else {
      (globalThis as Record<string, unknown>).window = originalWindow;
    }
  }
});
