import test from 'node:test';
import assert from 'node:assert/strict';

import {
  shouldForcePreserveWatcherForActiveSession,
  shouldApplyForegroundDetailHydration,
  shouldRestartWatchAfterInteractiveStream
} from '../../src/stores/chatWatchLifecycle';

test('watch restarts when stream finishes on the active session', () => {
  assert.equal(
    shouldRestartWatchAfterInteractiveStream({
      activeSessionId: 'sess_demo',
      targetSessionId: 'sess_demo',
      pageUnloading: false
    }),
    true
  );
});

test('watch does not restart when active session differs', () => {
  assert.equal(
    shouldRestartWatchAfterInteractiveStream({
      activeSessionId: 'sess_a',
      targetSessionId: 'sess_b',
      pageUnloading: false
    }),
    false
  );
});

test('watch does not restart while page is unloading', () => {
  assert.equal(
    shouldRestartWatchAfterInteractiveStream({
      activeSessionId: 'sess_demo',
      targetSessionId: 'sess_demo',
      pageUnloading: true
    }),
    false
  );
});

test('foreground detail hydration remains allowed when preserveWatcher mode only has watch controller', () => {
  assert.equal(
    shouldApplyForegroundDetailHydration({
      preserveWatcher: true,
      lifecycle: 'watching',
      hasWatchController: true,
      hasSendController: false,
      hasResumeController: false
    }),
    true
  );
});

test('foreground detail hydration is blocked while preserveWatcher mode still has interactive lifecycle', () => {
  assert.equal(
    shouldApplyForegroundDetailHydration({
      preserveWatcher: true,
      lifecycle: 'sending',
      hasWatchController: false,
      hasSendController: false,
      hasResumeController: false
    }),
    false
  );
});

test('foreground detail hydration is allowed once preserveWatcher mode is fully idle', () => {
  assert.equal(
    shouldApplyForegroundDetailHydration({
      preserveWatcher: true,
      lifecycle: 'idle',
      hasWatchController: false,
      hasSendController: false,
      hasResumeController: false
    }),
    true
  );
});

test('foreground detail hydration remains allowed when preserveWatcher mode is disabled', () => {
  assert.equal(
    shouldApplyForegroundDetailHydration({
      preserveWatcher: false,
      lifecycle: 'watching',
      hasWatchController: true,
      hasSendController: true,
      hasResumeController: true
    }),
    true
  );
});

test('foreground detail hydration can be forced for manual reconcile even during interactive lifecycle', () => {
  assert.equal(
    shouldApplyForegroundDetailHydration({
      preserveWatcher: true,
      lifecycle: 'sending',
      hasWatchController: true,
      hasSendController: true,
      hasResumeController: false,
      forceHydration: true
    }),
    true
  );
});

test('active-session detail load forces preserveWatcher while send stream is still alive', () => {
  assert.equal(
    shouldForcePreserveWatcherForActiveSession({
      isSameActiveSession: true,
      lifecycle: 'sending',
      hasSendController: false,
      hasResumeController: false
    }),
    true
  );
});

test('active-session detail load does not force preserveWatcher when only watch lifecycle is active', () => {
  assert.equal(
    shouldForcePreserveWatcherForActiveSession({
      isSameActiveSession: true,
      lifecycle: 'watching',
      hasSendController: false,
      hasResumeController: false
    }),
    false
  );
});

test('detail load never forces preserveWatcher for inactive sessions', () => {
  assert.equal(
    shouldForcePreserveWatcherForActiveSession({
      isSameActiveSession: false,
      lifecycle: 'sending',
      hasSendController: true,
      hasResumeController: false
    }),
    false
  );
});
