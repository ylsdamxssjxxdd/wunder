import test from 'node:test';
import assert from 'node:assert/strict';

import {
  resolveInteractiveControllerRecoveryReason,
  shouldRecoverInteractiveController
} from '../../src/stores/chatInteractiveRuntimeRecovery';

test('interactive controller recovery clears aborted controllers immediately', () => {
  assert.equal(
    resolveInteractiveControllerRecoveryReason({
      hasController: true,
      controllerAborted: true,
      nowMs: 1000
    }),
    'aborted'
  );
});

test('interactive controller recovery clears when remote runtime is already idle', () => {
  assert.equal(
    resolveInteractiveControllerRecoveryReason({
      hasController: true,
      controllerAborted: false,
      remoteRunning: false,
      nowMs: 1000
    }),
    'remote_idle'
  );
});

test('interactive controller recovery clears stale lock when remote event id keeps advancing', () => {
  assert.equal(
    shouldRecoverInteractiveController({
      hasController: true,
      controllerAborted: false,
      startedAt: 1000,
      lastEventAt: 1000,
      localLastEventId: 100,
      remoteLastEventId: 108,
      loading: true,
      nowMs: 9000
    }),
    true
  );
});

test('interactive controller recovery keeps active stream locks that still have recent activity', () => {
  assert.equal(
    shouldRecoverInteractiveController({
      hasController: true,
      controllerAborted: false,
      startedAt: 1000,
      lastEventAt: 7000,
      localLastEventId: 100,
      remoteLastEventId: 101,
      loading: true,
      nowMs: 9000
    }),
    false
  );
});

test('interactive controller recovery clears non-loading stale locks', () => {
  assert.equal(
    resolveInteractiveControllerRecoveryReason({
      hasController: true,
      controllerAborted: false,
      startedAt: 1000,
      lastEventAt: 2000,
      loading: false,
      nowMs: 16000
    }),
    'not_loading_stale'
  );
});

test('interactive controller recovery clears hard stale locks even without remote hints', () => {
  assert.equal(
    resolveInteractiveControllerRecoveryReason({
      hasController: true,
      controllerAborted: false,
      startedAt: 1000,
      lastEventAt: 2000,
      loading: true,
      nowMs: 95000
    }),
    'hard_stale'
  );
});
