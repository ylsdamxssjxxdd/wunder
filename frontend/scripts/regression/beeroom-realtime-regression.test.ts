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
import {
  resolveSyncRequiredReloadDelayMs,
  shouldRunSyncRequiredReloadImmediately
} from '../../src/components/beeroom/beeroomRealtimeSyncGap';
import { resolveNextRealtimeCursor } from '../../src/components/beeroom/beeroomRealtimeCursor';

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
  assert.equal(shouldForceWorkflowRefresh('team_task_update'), true);
  assert.equal(shouldForceWorkflowRefresh('team_task_result'), true);
  assert.equal(shouldForceWorkflowRefresh('team_finish'), true);
  assert.equal(shouldForceWorkflowRefresh(' team_error '), true);
  assert.equal(shouldForceWorkflowRefresh('team_start'), false);
});

test('team runtime reconcile forces immediate refresh for rejected or terminal events', () => {
  assert.equal(
    shouldForceImmediateTeamRealtimeReconcile({
      eventType: 'team_start',
      accepted: true
    }),
    true
  );
  assert.equal(
    shouldForceImmediateTeamRealtimeReconcile({
      eventType: 'team_task_dispatch',
      accepted: true
    }),
    true
  );
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
      eventType: 'team_task_update',
      accepted: true
    }),
    true
  );
});

test('sync_required reload throttle runs immediately only when window elapsed', () => {
  assert.equal(shouldRunSyncRequiredReloadImmediately(1500, 900, 520), true);
  assert.equal(shouldRunSyncRequiredReloadImmediately(1400, 900, 520), false);
  assert.equal(shouldRunSyncRequiredReloadImmediately(1500, 1500, 520), false);
  assert.equal(shouldRunSyncRequiredReloadImmediately('bad', 0, 520), false);
});

test('sync_required reload delay keeps minimum timer precision', () => {
  assert.equal(resolveSyncRequiredReloadDelayMs(1500, 900, 520), 0);
  assert.equal(resolveSyncRequiredReloadDelayMs(1400, 900, 520), 80);
  assert.equal(resolveSyncRequiredReloadDelayMs(1000, 900, 520), 420);
  assert.equal(resolveSyncRequiredReloadDelayMs(900, 1000, 520), 520);
});

test('realtime cursor advances by stream event id and never rolls back', () => {
  assert.equal(
    resolveNextRealtimeCursor({
      currentCursor: 15,
      eventId: '18',
      payload: { event_id: 17 }
    }),
    18
  );
  assert.equal(
    resolveNextRealtimeCursor({
      currentCursor: 18,
      eventId: '16',
      payload: { event_id: 12 }
    }),
    18
  );
});

test('realtime cursor accepts payload resume fields for reconnect continuity', () => {
  assert.equal(
    resolveNextRealtimeCursor({
      currentCursor: 0,
      eventId: '',
      payload: { after_event_id: 21 }
    }),
    21
  );
  assert.equal(
    resolveNextRealtimeCursor({
      currentCursor: 21,
      eventId: '',
      payload: { latest_event_id: 29 }
    }),
    29
  );
});
