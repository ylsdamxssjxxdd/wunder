import test from 'node:test';
import assert from 'node:assert/strict';
import { ref } from 'vue';

import { buildTaskList, isArchivedTaskRecord, isRootWorkThread, taskWindow } from '../../src/views/messenger/taskList';
import { useTaskListActivity } from '../../src/views/messenger/useTaskListActivity';
import { moveKeyWithinOrder, prependFreshKeys } from '../../src/views/messenger/stableListOrder';
import { resolveTaskRuntimeState } from '../../src/views/messenger/taskRuntimeState';
import { applySessionQuotaUsage } from '../../src/stores/chatSessionQuota';
import { mergeSessionRuntimeFields } from '../../src/stores/chatSessionMerge';

test('work thread list excludes archived and subagent sessions', () => {
  assert.equal(isArchivedTaskRecord({ status: 'archived' }), true);
  assert.equal(isArchivedTaskRecord({ archived_at: '2026-01-01T00:00:00Z' }), true);
  assert.equal(isRootWorkThread({ id: 'root' }), true);
  assert.equal(isRootWorkThread({ parent_session_id: 'parent', spawned_by: 'model' }), false);
  assert.deepEqual(
    buildTaskList([
      { id: 'active', agent_id: 'agent', status: 'active', created_at: 2 },
      { id: 'archived', agent_id: 'agent', status: 'archived', created_at: 3 },
      { id: 'child', agent_id: 'agent', parent_session_id: 'root', spawned_by: 'model', created_at: 4 }
    ], 'agent', 'New'),
    [{ id: 'active', title: 'New', locked: false, runtimeStatus: 'active', createdAt: 2, consumedTokens: 0, toolCalls: 0, quotaUsed: null }]
  );
});

test('thread drag order uses the complete persisted order', () => {
  assert.deepEqual(
    moveKeyWithinOrder(['a', 'b', 'c', 'd'], 'd', 'a', 'before'),
    ['d', 'a', 'b', 'c']
  );
  assert.deepEqual(
    moveKeyWithinOrder(['a', 'b', 'c', 'd'], 'a', 'd', 'after'),
    ['b', 'c', 'd', 'a']
  );
});

test('work catalog preserves explicit forks and swarm threads', () => {
  for (const source of ['thread_control', 'agent_swarm']) {
    assert.equal(isRootWorkThread({ parent_session_id: 'parent', spawned_by: source }), true);
  }
});

test('newer threads are promoted above a manually ordered older thread', () => {
  const timestamps = new Map([
    ['a', 100],
    ['b', 200],
    ['n', 300]
  ]);
  assert.deepEqual(
    prependFreshKeys(['a', 'n', 'b'], ['a', 'b'], ['a', 'n', 'b'], timestamps),
    ['n', 'a', 'b']
  );
  assert.deepEqual(
    prependFreshKeys(['a', 'old', 'b'], ['a', 'b'], ['a', 'old', 'b'], new Map([
      ['a', 100], ['b', 200], ['old', 50]
    ])),
    ['a', 'old', 'b']
  );
});

test('thread quota uses server totals independently of tokens, message history and replay', () => {
  const sessions = [{ id: 'thread', consumed_tokens: 9000, quota_used: 0 }];
  assert.equal(applySessionQuotaUsage(sessions, 'thread', { session_quota_used: 3, consumed: 1 }), true);
  assert.equal(applySessionQuotaUsage(sessions, 'thread', { session_quota_used: 3, consumed: 1 }), false);
  assert.equal(applySessionQuotaUsage(sessions, 'thread', { session_quota_used: 2 }), false);
  assert.equal(applySessionQuotaUsage(sessions, 'thread', { used: 999, consumed: 1 }), false);
  assert.deepEqual(sessions, [{ id: 'thread', consumed_tokens: 9000, quota_used: 3 }]);
  assert.deepEqual(mergeSessionRuntimeFields(sessions[0], { quota_used: 1 }), sessions[0]);
  assert.deepEqual(buildTaskList([
    { id: 'a', quota_used: 0 }, { id: 'b', quota_used: 1200 }, { id: 'c', consumed_tokens: 1234 }
  ], '', '').map(item => item.quotaUsed), [0, 1200, null]);
});

test('thread icons honor live settlement, queue priority and terminal state over stale loading', () => {
  assert.deepEqual([
    resolveTaskRuntimeState('idle', 'running', false),
    resolveTaskRuntimeState('not_loaded', 'pending', true),
    resolveTaskRuntimeState('waiting_approval', 'running', true),
    resolveTaskRuntimeState('completed', 'running', true),
    resolveTaskRuntimeState('failed', 'running', true),
    resolveTaskRuntimeState('cancelled', 'running', false),
    resolveTaskRuntimeState('finalizing', 'idle', false),
    resolveTaskRuntimeState('not_loaded', 'active', true)
  ], ['idle', 'pending', 'pending', 'done', 'error', 'done', 'running', 'running']);
});

test('activity filter finds offscreen threads and follows settlement without losing the full order', () => {
  const items = ref(Array.from({ length: 100 }, (_, index) => ({ id: `thread-${index}`, status: 'idle' })));
  items.value[90].status = 'running';
  items.value[95].status = 'queued';
  items.value[96].status = 'waiting_approval';
  items.value[97].status = 'waiting_user_input';
  const list = useTaskListActivity(items, (item) => resolveTaskRuntimeState(item.status, '', false));
  assert.equal(list.activeCount.value, 4);
  assert.equal(list.activityState.value, 'running');
  assert.equal(list.displayItems.value, items.value);
  list.showActiveOnly.value = true;
  assert.deepEqual(list.displayItems.value.map(item => item.id), ['thread-90', 'thread-95', 'thread-96', 'thread-97']);
  assert.deepEqual(taskWindow(list.displayItems.value.length, 4000, 300, 54), { start: 0, end: 4 });

  items.value[90].status = 'completed';
  assert.equal(list.activeCount.value, 3);
  assert.equal(list.activityState.value, 'pending');
  items.value[95].status = 'failed';
  items.value[96].status = 'cancelled';
  items.value[97].status = 'idle';
  assert.equal(list.activeCount.value, 0);
  assert.deepEqual(list.displayItems.value, []);
  // Keep the filter explicit even after the final active thread settles.
  assert.equal(list.showActiveOnly.value, true);
  items.value[99].status = 'resuming';
  assert.deepEqual(list.displayItems.value.map(item => item.id), ['thread-99']);
  list.showActiveOnly.value = false;
  assert.equal(list.displayItems.value, items.value);
});

test('dragging between filtered rows moves stable IDs in the full thread order', () => {
  const items = ref([
    { id: 'a', status: 'idle' }, { id: 'b', status: 'running' },
    { id: 'c', status: 'idle' }, { id: 'd', status: 'running' }
  ]);
  const list = useTaskListActivity(items, (item) => resolveTaskRuntimeState(item.status, '', false));
  list.showActiveOnly.value = true;
  const [target, dragged] = list.displayItems.value;
  const order = moveKeyWithinOrder(items.value.map(item => item.id), dragged.id, target.id, 'before');
  assert.deepEqual(order, ['a', 'd', 'b', 'c']);
  items.value = order.map(id => items.value.find(item => item.id === id)!);
  assert.deepEqual(list.displayItems.value.map(item => item.id), ['d', 'b']);
  list.showActiveOnly.value = false;
  assert.deepEqual(list.displayItems.value.map(item => item.id), ['a', 'd', 'b', 'c']);
});
