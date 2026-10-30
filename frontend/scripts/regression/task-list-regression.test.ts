import test from 'node:test';
import assert from 'node:assert/strict';
import { ref } from 'vue';

import { buildTaskList, isArchivedTaskRecord, isRootWorkThread, taskWindow } from '../../src/views/messenger/taskList';
import { formatDateTime, toEpochMs } from '../../src/utils/formatDateTime';
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
    [{ id: 'active', title: 'New', locked: false, runtimeStatus: 'active', createdAt: 2, lastActiveAt: 2, consumedTokens: 0, toolCalls: 0, modelRequestCount: null, quotaUsed: null }]
  );
});

test('work thread rows carry the newest activity time for the row tooltip', () => {
  const rows = buildTaskList([
    {
      id: 'chatty',
      agent_id: 'agent',
      created_at: '2026-01-01T08:00:00Z',
      updated_at: '2026-01-02T09:00:00Z',
      last_message_at: '2026-01-03T10:30:00Z'
    },
    // No message stamp: the rename-driven updated_at is the best activity
    // signal this row has, so the tooltip still gets a time.
    { id: 'renamed', agent_id: 'agent', created_at: '2026-01-01T08:00:00Z', updated_at: '2026-02-01T09:00:00Z' }
  ], 'agent', 'New');

  const chatty = rows.find((row) => row.id === 'chatty');
  assert.equal(chatty?.lastActiveAt, Date.parse('2026-01-03T10:30:00Z'));
  const renamed = rows.find((row) => row.id === 'renamed');
  assert.equal(renamed?.lastActiveAt, Date.parse('2026-02-01T09:00:00Z'));
});

test('tooltip timestamp accepts seconds, milliseconds and ISO text', () => {
  const ms = Date.parse('2026-03-04T05:06:00Z');
  assert.equal(toEpochMs(ms / 1000), ms);
  assert.equal(toEpochMs(ms), ms);
  assert.equal(toEpochMs('2026-03-04T05:06:00Z'), ms);
  assert.equal(toEpochMs(''), 0);
  assert.equal(toEpochMs(0), 0);
  // Local-time label, so it is built from the same Date the assertion reads.
  const label = new Date(ms);
  const pad = (part: number) => String(part).padStart(2, '0');
  assert.equal(
    formatDateTime(ms),
    `${label.getFullYear()}-${pad(label.getMonth() + 1)}-${pad(label.getDate())} ${pad(label.getHours())}:${pad(label.getMinutes())}`
  );
  assert.equal(formatDateTime(0), '');
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

test('thread model request counts use server totals independently of tokens, credits and replay', () => {
  const sessions = [{ id: 'thread', consumed_tokens: 9000, quota_used: 0 }];
  assert.equal(applySessionQuotaUsage(sessions, 'thread', {
    session_request_count: 3, session_quota_used: 999, consumed: 1, account_credits_consumed: 42
  }), true);
  assert.equal(applySessionQuotaUsage(sessions, 'thread', { session_request_count: 3 }), false);
  // Historical events carry the same absolute model count in session_quota_used.
  assert.equal(applySessionQuotaUsage(sessions, 'thread', { session_quota_used: 3, consumed: 1 }), false);
  assert.equal(applySessionQuotaUsage(sessions, 'thread', { session_quota_used: 2 }), false);
  assert.equal(applySessionQuotaUsage(sessions, 'thread', { used: 999, consumed: 1 }), false);
  assert.deepEqual(sessions, [{ id: 'thread', consumed_tokens: 9000, model_request_count: 3, quota_used: 3 }]);
  assert.deepEqual(mergeSessionRuntimeFields(sessions[0], { quota_used: 1 }), sessions[0]);
  assert.deepEqual(buildTaskList([
    { id: 'a', model_request_count: 0, quota_used: 5 },
    { id: 'b', quota_used: 1200 },
    { id: 'c', consumed_tokens: 1234, account_credits_consumed: 42 }
  ], '', '').map(item => [item.modelRequestCount, item.quotaUsed]), [[0, 0], [1200, 1200], [null, null]]);
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
  ], ['idle', 'pending', 'pending', 'done', 'error', 'idle', 'running', 'running']);
  assert.equal(resolveTaskRuntimeState('idle', 'idle', false, 'idle', true), 'running');
  // A background thread can commit its terminal change while its original
  // request stream is still being torn down after the user switched threads.
  // The durable result must settle the task row immediately.
  assert.equal(resolveTaskRuntimeState('completed', 'running', true, 'running', true), 'done');
  assert.equal(resolveTaskRuntimeState('failed', 'running', true, 'running', true), 'error');
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
