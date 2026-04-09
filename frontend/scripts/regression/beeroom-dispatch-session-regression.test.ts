import test from 'node:test';
import assert from 'node:assert/strict';

import {
  resolveNextBeeroomMotherDispatchSessionId,
  resolvePreferredBeeroomDispatchSessionId,
  shouldFinishBeeroomTerminalHydration,
  shouldPreserveBeeroomDispatchPreviewOnSyncError
} from '../../src/components/beeroom/beeroomDispatchSessionPolicy';

test('mother dispatch keeps following current primary session instead of stale previous session', () => {
  const sessionId = resolvePreferredBeeroomDispatchSessionId({
    targetRole: 'mother',
    targetAgentId: 'mother-1',
    previousSessionId: 'sess-old',
    previousTargetAgentId: 'mother-1',
    activeSessionId: 'sess-active',
    primarySessionId: 'sess-main',
    hasExplicitPrimarySession: true
  });
  assert.equal(sessionId, 'sess-main');
});

test('mother dispatch keeps the bound session when explicit main thread is temporarily unavailable', () => {
  const sessionId = resolvePreferredBeeroomDispatchSessionId({
    targetRole: 'mother',
    targetAgentId: 'mother-1',
    previousSessionId: 'sess-old',
    previousTargetAgentId: 'mother-1',
    activeSessionId: 'sess-active',
    primarySessionId: '',
    hasExplicitPrimarySession: false
  });
  assert.equal(sessionId, 'sess-old');
});

test('mother dispatch stays on the bound session while explicit main thread is still unknown', () => {
  const sessionId = resolvePreferredBeeroomDispatchSessionId({
    targetRole: 'mother',
    targetAgentId: 'mother-1',
    previousSessionId: 'sess-current',
    previousTargetAgentId: 'mother-1',
    activeSessionId: 'sess-active',
    primarySessionId: 'sess-main-candidate',
    hasExplicitPrimarySession: false
  });
  assert.equal(sessionId, 'sess-current');
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

test('mother reconcile keeps the current mother session until an explicit main thread exists', () => {
  assert.equal(
    resolveNextBeeroomMotherDispatchSessionId({
      motherAgentId: 'mother-1',
      currentSessionId: 'sess-current',
      currentSessionAgentId: 'mother-1',
      explicitPrimarySessionId: '',
      fallbackPrimarySessionId: 'sess-main-candidate'
    }),
    'sess-current'
  );
  assert.equal(
    resolveNextBeeroomMotherDispatchSessionId({
      motherAgentId: 'mother-1',
      currentSessionId: 'sess-current',
      currentSessionAgentId: 'mother-1',
      explicitPrimarySessionId: 'sess-main',
      fallbackPrimarySessionId: 'sess-main-candidate'
    }),
    'sess-main'
  );
});

test('terminal beeroom hydration waits for the expected final reply instead of any assistant signature drift', () => {
  assert.equal(
    shouldFinishBeeroomTerminalHydration({
      expectedReplyText: '最终回复',
      expectedReplyMatched: false,
      baselineAssistantSignature: '1|10|k1|中间回复',
      assistantSignature: '2|12|k2|中间回复'
    }),
    false
  );
  assert.equal(
    shouldFinishBeeroomTerminalHydration({
      expectedReplyText: '最终回复',
      expectedReplyMatched: true,
      baselineAssistantSignature: '1|10|k1|中间回复',
      assistantSignature: '3|15|k3|最终回复'
    }),
    true
  );
  assert.equal(
    shouldFinishBeeroomTerminalHydration({
      expectedReplyText: '',
      expectedReplyMatched: false,
      baselineAssistantSignature: '1|10|k1|旧回复',
      assistantSignature: '2|12|k2|新回复'
    }),
    true
  );
});
