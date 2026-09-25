import test from 'node:test';
import assert from 'node:assert/strict';

import { mergeSessionsByIdPreservingRuntimeFields } from '../../src/stores/chatSessionMerge';
import { applySessionQuotaUsage } from '../../src/stores/chatSessionQuota';

test('session merge keeps locally enhanced context fields when server refresh omits them', () => {
  const currentSessions = [
    {
      id: 'sess_a',
      title: 'Current',
      contextTokens: 4583,
      contextTotalTokens: 32768,
      goal: { objective: 'persist' }
    }
  ];
  const incomingSessions = [
    {
      id: 'sess_a',
      title: 'Current',
      updated_at: '2026-05-09T10:00:00.000Z'
    }
  ];

  const merged = mergeSessionsByIdPreservingRuntimeFields(
    currentSessions,
    incomingSessions,
    (session) => ({ ...(session || {}) }),
    (sessions) => sessions
  );

  assert.equal(merged.length, 1);
  assert.equal(merged[0].contextTokens, 4583);
  assert.equal(merged[0].contextTotalTokens, 32768);
  assert.deepEqual(merged[0].goal, { objective: 'persist' });
  assert.equal(merged[0].updated_at, '2026-05-09T10:00:00.000Z');
});

test('session request count survives live model request usage and later stale list refresh', () => {
  const sessions = [{ id: 'sess_a', model_request_count: 2, quota_used: 2 }];
  assert.equal(applySessionQuotaUsage(sessions, 'sess_a', {
    session_request_count: 3,
    request_count: 1
  }), true);
  const merged = mergeSessionsByIdPreservingRuntimeFields(
    sessions,
    [{ id: 'sess_a', model_request_count: 2, quota_used: 2 }],
    (session) => ({ ...(session || {}) }),
    (items) => items
  );
  assert.equal(merged[0].model_request_count, 3);
  assert.equal(merged[0].quota_used, 3);
});
