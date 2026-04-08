import test from 'node:test';
import assert from 'node:assert/strict';

import {
  resolvePreferredBeeroomDispatchSessionId,
  shouldPreserveBeeroomDispatchPreviewOnSyncError
} from '../../src/components/beeroom/beeroomDispatchSessionPolicy';

test('mother dispatch keeps following current primary session instead of stale previous session', () => {
  const sessionId = resolvePreferredBeeroomDispatchSessionId({
    targetRole: 'mother',
    targetAgentId: 'mother-1',
    previousSessionId: 'sess-old',
    previousTargetAgentId: 'mother-1',
    activeSessionId: 'sess-active',
    primarySessionId: 'sess-main'
  });
  assert.equal(sessionId, 'sess-main');
});

test('mother dispatch falls back to active session when primary session is temporarily unavailable', () => {
  const sessionId = resolvePreferredBeeroomDispatchSessionId({
    targetRole: 'mother',
    targetAgentId: 'mother-1',
    previousSessionId: 'sess-old',
    previousTargetAgentId: 'mother-1',
    activeSessionId: 'sess-active',
    primarySessionId: ''
  });
  assert.equal(sessionId, 'sess-active');
});

test('worker dispatch only reuses previous session when it belongs to the same worker target', () => {
  assert.equal(
    resolvePreferredBeeroomDispatchSessionId({
      targetRole: 'worker',
      targetAgentId: 'worker-2',
      previousSessionId: 'sess-worker-2',
      previousTargetAgentId: 'worker-2',
      activeSessionId: 'sess-active-worker-2',
      primarySessionId: 'sess-main-worker-2'
    }),
    'sess-worker-2'
  );
  assert.equal(
    resolvePreferredBeeroomDispatchSessionId({
      targetRole: 'worker',
      targetAgentId: 'worker-2',
      previousSessionId: 'sess-other-worker',
      previousTargetAgentId: 'worker-1',
      activeSessionId: 'sess-active-worker-2',
      primarySessionId: 'sess-main-worker-2'
    }),
    'sess-active-worker-2'
  );
});

test('dispatch preview is preserved only for transient sync errors on the same session', () => {
  assert.equal(
    shouldPreserveBeeroomDispatchPreviewOnSyncError({
      status: 0,
      currentPreviewSessionId: 'sess_1',
      requestedSessionId: 'sess_1'
    }),
    true
  );
  assert.equal(
    shouldPreserveBeeroomDispatchPreviewOnSyncError({
      status: 500,
      currentPreviewSessionId: 'sess_1',
      requestedSessionId: 'sess_1'
    }),
    true
  );
  assert.equal(
    shouldPreserveBeeroomDispatchPreviewOnSyncError({
      status: 404,
      currentPreviewSessionId: 'sess_1',
      requestedSessionId: 'sess_1'
    }),
    false
  );
  assert.equal(
    shouldPreserveBeeroomDispatchPreviewOnSyncError({
      status: 500,
      currentPreviewSessionId: 'sess_old',
      requestedSessionId: 'sess_new'
    }),
    false
  );
});
