import test from 'node:test';
import assert from 'node:assert/strict';
import {
  applyLocalQueueReorder,
  isChatQueueRuntimeEvent,
  projectQueueItems,
  type QueuedTurn
} from '../../src/stores/chatQueueProjection';

const parked = (queueId: string, content: string, createdAt: number): QueuedTurn =>
  projectQueueItems([{ queue_id: queueId, content, created_at: createdAt, position: createdAt }])[0];

test('projectQueueItems 丢弃无效行并按派发序补齐位次', () => {
  const items = projectQueueItems([
    { queue_id: 'task-b', content: '哈哈', created_at: 200, position: 1 },
    { queue_id: 'task-a', content: '额', created_at: 100, position: 0 },
    { queue_id: '   ', content: 'ignored', created_at: 50 },
    null,
    'not-an-object'
  ]);
  assert.deepEqual(items.map((item) => item.queueId), ['task-a', 'task-b']);
  assert.deepEqual(items.map((item) => item.position), [0, 1]);
  assert.equal(items[0].content, '额');
  assert.equal(items[0].status, 'pending');
});

test('projectQueueItems 让刚插话的条目排到最前', () => {
  const items = projectQueueItems([
    { queueId: 'task-a', createdAt: 100, position: 0, priority: 0 },
    { queueId: 'task-b', createdAt: 200, position: 1, priority: 1 }
  ]);
  assert.deepEqual(items.map((item) => item.queueId), ['task-b', 'task-a']);
});

test('applyLocalQueueReorder 保留用户给定的顺序，且只在完整覆盖时生效', () => {
  const items = [parked('task-a', 'first', 100), parked('task-b', 'second', 200), parked('task-c', 'third', 300)];
  const next = applyLocalQueueReorder(items, ['task-c', 'task-a', 'task-b']);
  assert.ok(next);
  assert.deepEqual(next?.map((item) => item.queueId), ['task-c', 'task-a', 'task-b']);
  assert.deepEqual(next?.map((item) => item.position), [0, 1, 2]);
  assert.equal(applyLocalQueueReorder(items, ['task-c', 'task-a']), null);
  assert.equal(applyLocalQueueReorder(items, ['task-c', 'task-a', 'unknown']), null);
});

test('isChatQueueRuntimeEvent 只认排队相关事件', () => {
  assert.ok(isChatQueueRuntimeEvent('queue_enter'));
  assert.ok(isChatQueueRuntimeEvent(' Queue_Update '));
  assert.ok(isChatQueueRuntimeEvent('queue_cancel'));
  assert.ok(isChatQueueRuntimeEvent('queued'));
  assert.equal(isChatQueueRuntimeEvent('llm_output_delta'), false);
  assert.equal(isChatQueueRuntimeEvent('thread_status'), false);
  assert.equal(isChatQueueRuntimeEvent(''), false);
});
