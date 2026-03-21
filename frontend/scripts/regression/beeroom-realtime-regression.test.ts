import test from 'node:test';
import assert from 'node:assert/strict';

import {
  isStaleRealtimeUpdate,
  shouldApplyRealtimeStatusTransition
} from '../../src/stores/beeroomRealtimeStatus';

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

