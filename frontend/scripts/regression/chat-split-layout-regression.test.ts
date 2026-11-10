import test from 'node:test';
import assert from 'node:assert/strict';
import {
  assignChatSplitSlot,
  clampChatSplitRatio,
  closeChatSplitSlot,
  createChatSplitSlots,
  splitSlotAtPoint
} from '../../src/views/messenger/chatSplitLayout';

test('split slots start with the active thread and blank drop targets', () => {
  assert.deepEqual(createChatSplitSlots(1, 'a'), []);
  assert.deepEqual(createChatSplitSlots(2, 'a'), ['a', '']);
  assert.deepEqual(createChatSplitSlots(4, 'a'), ['a', '', '', '']);
});

test('drop assignment replaces a pane and moves an already visible thread', () => {
  assert.deepEqual(assignChatSplitSlot([], 1, 'b', 'a'), ['a', 'b']);
  assert.deepEqual(assignChatSplitSlot(['a', 'b'], 1, 'c'), ['a', 'c']);
  assert.deepEqual(assignChatSplitSlot(['a', 'b', 'c', ''], 2, 'b'), ['a', 'c', 'b', '']);
  assert.deepEqual(assignChatSplitSlot(['a', 'b', 'c', ''], 3, 'b'), ['a', 'b', 'c', '']);
});

test('closing compacts slots and reduces the layout before the next drop', () => {
  assert.deepEqual(closeChatSplitSlot(['a', 'b', 'c', ''], 1), ['a', 'c']);
  assert.deepEqual(closeChatSplitSlot(['a', 'b', 'c', 'd'], 2), ['a', 'b', 'd', '']);
  assert.deepEqual(closeChatSplitSlot(['a', ''], 1), []);
  assert.deepEqual(closeChatSplitSlot(['a', ''], 0), []);
  assert.deepEqual(
    assignChatSplitSlot(closeChatSplitSlot(['a', 'b', 'c', ''], 1), 2, 'd'),
    ['a', 'c', 'd']
  );
});

test('ratios are bounded and hit testing covers two-pane and four-grid layouts', () => {
  assert.equal(clampChatSplitRatio(-1), 0.25);
  assert.equal(clampChatSplitRatio(2), 0.75);
  assert.equal(clampChatSplitRatio(Number.NaN), 0.5);
  assert.equal(splitSlotAtPoint(2, 0.2, 0.9), 0);
  assert.equal(splitSlotAtPoint(2, 0.8, 0.1), 1);
  assert.equal(splitSlotAtPoint(4, 0.8, 0.8), 3);
  assert.equal(splitSlotAtPoint(4, 0.2, 0.8), 2);
});
