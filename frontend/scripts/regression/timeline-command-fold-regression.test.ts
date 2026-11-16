// AI生成 · 工作过程条目手动开合回归：运行状态不改变页面高度。
import test from 'node:test';
import assert from 'node:assert/strict';

import { resolveEntryOpen } from '../../src/components/chat/timelineCommandFold';

test('运行中的命令默认收起，状态推进不会自动展开', () => {
  for (const open of [new Set<string>(), [] as string[]]) {
    assert.equal(resolveEntryOpen({ key: 'run-1', open }), false);
  }
});

test('命令结束不会收掉用户已经展开的内容', () => {
  assert.equal(resolveEntryOpen({ key: 'run-1', open: new Set(['run-1']) }), true);
  assert.equal(resolveEntryOpen({ key: 'run-1', open: ['run-1'] }), true);
});

test('用户手动收起后保持收起', () => {
  assert.equal(resolveEntryOpen({ key: 'run-1', open: new Set(['other']) }), false);
});
