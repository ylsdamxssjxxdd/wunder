import test from 'node:test';
import assert from 'node:assert/strict';

import { settleAgentSessionBusyAfterRefresh } from '../../src/views/messenger/chatRefreshRecovery';

test('refresh recovery skips when session is already idle', async () => {
  let loadCalls = 0;
  const status = await settleAgentSessionBusyAfterRefresh({
    sessionId: 'sess_a',
    isSessionBusy: () => false,
    resolveRuntimeStatus: () => 'idle',
    loadSessionDetail: async () => {
      loadCalls += 1;
    }
  });
  assert.equal(status, 'already_idle');
  assert.equal(loadCalls, 0);
});

test('refresh recovery keeps genuine runtime busy sessions after forced reconciliation', async () => {
  let loadCalls = 0;
  const status = await settleAgentSessionBusyAfterRefresh({
    sessionId: 'sess_a',
    isSessionBusy: () => true,
    resolveRuntimeStatus: () => 'running',
    loadSessionDetail: async () => {
      loadCalls += 1;
    },
    attempts: 1,
    settleDelayMs: 0
  });
  assert.equal(status, 'runtime_busy');
  assert.equal(loadCalls, 1);
});

test('refresh recovery can clear stale runtime busy state after forced reconciliation', async () => {
  let loadCalls = 0;
  let busy = true;
  let runtimeStatus = 'running';
  const status = await settleAgentSessionBusyAfterRefresh({
    sessionId: 'sess_a',
    isSessionBusy: () => busy,
    resolveRuntimeStatus: () => runtimeStatus,
    loadSessionDetail: async () => {
      loadCalls += 1;
      busy = false;
      runtimeStatus = 'idle';
    },
    attempts: 2,
    settleDelayMs: 0
  });
  assert.equal(status, 'settled');
  assert.equal(loadCalls, 1);
});

test('refresh recovery can settle stale busy session after hard hydration', async () => {
  let loadCalls = 0;
  let busy = true;
  const status = await settleAgentSessionBusyAfterRefresh({
    sessionId: 'sess_a',
    isSessionBusy: () => busy,
    resolveRuntimeStatus: () => 'idle',
    loadSessionDetail: async () => {
      loadCalls += 1;
      busy = false;
    },
    attempts: 2,
    settleDelayMs: 0
  });
  assert.equal(status, 'settled');
  assert.equal(loadCalls, 1);
});

test('refresh recovery reports unsettled when stale busy cannot be cleared', async () => {
  let loadCalls = 0;
  const status = await settleAgentSessionBusyAfterRefresh({
    sessionId: 'sess_a',
    isSessionBusy: () => true,
    resolveRuntimeStatus: () => 'idle',
    loadSessionDetail: async () => {
      loadCalls += 1;
    },
    attempts: 3,
    settleDelayMs: 0
  });
  assert.equal(status, 'unsettled');
  assert.equal(loadCalls, 3);
});
