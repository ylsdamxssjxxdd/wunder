import test from 'node:test';
import assert from 'node:assert/strict';

import {
  isStaleRealtimeUpdate,
  shouldApplyRealtimeStatusTransition
} from '../../src/stores/beeroomRealtimeStatus';
import {
  shouldForceImmediateTeamRealtimeReconcile,
  shouldForceWorkflowRefresh
} from '../../src/components/beeroom/beeroomRealtimeReconcile';

const terminalTaskStatuses = new Set(['success', 'completed', 'failed', 'error', 'timeout', 'cancelled']);

const isTerminalTaskStatus = (status: string): boolean =>
  terminalTaskStatuses.has(String(status || '').trim().toLowerCase());

test('stale realtime update is rejected by timestamp', () => {
  assert.equal(isStaleRealtimeUpdate(120, 119), true);
  assert.equal(isStaleRealtimeUpdate(120, 120), false);
  assert.equal(isStaleRealtimeUpdate(120, 121), false);
});

test('terminal task status is not downgraded by fresh non-terminal event', () => {
  const accepted = shouldApplyRealtimeStatusTransition({
    currentStatus: 'completed',
    currentUpdatedTime: 100,
    incomingStatus: 'running',
    incomingUpdatedTime: 100,
    isTerminalStatus: isTerminalTaskStatus
  });
  assert.equal(accepted, false);
});

test('non-terminal task status accepts fresh realtime transition', () => {
  const accepted = shouldApplyRealtimeStatusTransition({
    currentStatus: 'running',
    currentUpdatedTime: 100,
    incomingStatus: 'completed',
    incomingUpdatedTime: 101,
    isTerminalStatus: isTerminalTaskStatus
  });
  assert.equal(accepted, true);
});

test('stale terminal update is still rejected to avoid rollback jitter', () => {
  const accepted = shouldApplyRealtimeStatusTransition({
    currentStatus: 'running',
    currentUpdatedTime: 150,
    incomingStatus: 'completed',
    incomingUpdatedTime: 149,
    isTerminalStatus: isTerminalTaskStatus
  });
  assert.equal(accepted, false);
});

test('terminal task status can accept fresh terminal transition', () => {
  const accepted = shouldApplyRealtimeStatusTransition({
    currentStatus: 'failed',
    currentUpdatedTime: 200,
    incomingStatus: 'completed',
    incomingUpdatedTime: 201,
    isTerminalStatus: isTerminalTaskStatus
  });
  assert.equal(accepted, true);
});

test('empty incoming status is ignored', () => {
  const accepted = shouldApplyRealtimeStatusTransition({
    currentStatus: 'running',
    currentUpdatedTime: 300,
    incomingStatus: '',
    incomingUpdatedTime: 301,
    isTerminalStatus: isTerminalTaskStatus
  });
  assert.equal(accepted, false);
});

test('team runtime event marks workflow refresh only for runtime-critical types', () => {
  assert.equal(shouldForceWorkflowRefresh('team_task_dispatch'), true);
  assert.equal(shouldForceWorkflowRefresh('team_task_result'), true);
  assert.equal(shouldForceWorkflowRefresh('team_finish'), true);
  assert.equal(shouldForceWorkflowRefresh(' team_error '), true);
  assert.equal(shouldForceWorkflowRefresh('team_start'), false);
});

test('team runtime reconcile forces immediate refresh for rejected or terminal events', () => {
  assert.equal(
    shouldForceImmediateTeamRealtimeReconcile({
      eventType: 'team_task_result',
      accepted: false
    }),
    true
  );
  assert.equal(
    shouldForceImmediateTeamRealtimeReconcile({
      eventType: 'team_finish',
      accepted: true
    }),
    true
  );
  assert.equal(
    shouldForceImmediateTeamRealtimeReconcile({
      eventType: 'team_error',
      accepted: true
    }),
    true
  );
  assert.equal(
    shouldForceImmediateTeamRealtimeReconcile({
      eventType: 'team_task_dispatch',
      accepted: true
    }),
    false
  );
});
