import test from 'node:test';
import assert from 'node:assert/strict';

import { shouldWatchdogReconcileDrift } from '../../src/stores/chatWatchdogRecovery';

test('watchdog reconciles drift after stream stops when server event id is newer', () => {
  assert.equal(
    shouldWatchdogReconcileDrift({
      remoteLastEventId: 19,
      localLastEventId: 17,
      hasPendingMessage: false
    }),
    true
  );
});

test('watchdog does not reconcile while a pending assistant should resume instead', () => {
  assert.equal(
    shouldWatchdogReconcileDrift({
      remoteLastEventId: 19,
      localLastEventId: 17,
      hasPendingMessage: true
    }),
    false
  );
});

test('watchdog ignores non-advancing or invalid event ids', () => {
  assert.equal(
    shouldWatchdogReconcileDrift({
      remoteLastEventId: 17,
      localLastEventId: 17,
      hasPendingMessage: false
    }),
    false
  );
  assert.equal(
    shouldWatchdogReconcileDrift({
      remoteLastEventId: 'not-a-number',
      localLastEventId: 17,
      hasPendingMessage: false
    }),
    false
  );
});

test('watchdog ignores single-step terminal drift after hydrate', () => {
  assert.equal(
    shouldWatchdogReconcileDrift({
      remoteLastEventId: 15,
      localLastEventId: 14,
      hasPendingMessage: false
    }),
    false
  );
});

test('watchdog still reconciles meaningful drift after hydrate', () => {
  assert.equal(
    shouldWatchdogReconcileDrift({
      remoteLastEventId: 16,
      localLastEventId: 14,
      hasPendingMessage: false
    }),
    true
  );
});
