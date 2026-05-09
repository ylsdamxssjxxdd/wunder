import test from 'node:test';
import assert from 'node:assert/strict';

import { mergeSessionsByIdPreservingRuntimeFields } from '../../src/stores/chatSessionMerge';

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
