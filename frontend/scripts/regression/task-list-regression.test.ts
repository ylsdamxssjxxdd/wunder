import test from 'node:test';
import assert from 'node:assert/strict';

import { buildTaskList, isArchivedTaskRecord, isRootWorkThread } from '../../src/views/messenger/taskList';
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
