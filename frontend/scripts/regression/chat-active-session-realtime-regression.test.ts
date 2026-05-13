import test from 'node:test';
import assert from 'node:assert/strict';

import {
  INTERACTIVE_STREAM_RECONCILE_IDLE_MS,
  resolveActiveSessionRealtimeRecoveryPlan,
  shouldStartWatcherAfterSessionHydration,
  shouldReconcileInteractiveStream
} from '../../src/stores/chatActiveSessionRealtime';
import { shouldShowAgentSettingsPanelForSection } from '../../src/views/messenger/settingsPanelVisibility';

test('active session realtime recovery starts watch after returning to messages', () => {
  assert.equal(
    resolveActiveSessionRealtimeRecoveryPlan({
      targetSessionId: 'session_demo',
      activeSessionId: 'session_demo',
      hasWatchController: false,
      hasSendController: false,
      hasResumeController: false,
      loading: false,
      hasPendingAssistant: true,
      hasRunningAssistant: true,
      hydrateIfCold: true
    }),
    'watch'
  );
});

test('active session realtime recovery watches a queued placeholder after send ack', () => {
  assert.equal(
    resolveActiveSessionRealtimeRecoveryPlan({
      targetSessionId: 'session_demo',
      activeSessionId: 'session_demo',
      hasWatchController: false,
      hasSendController: false,
      hasResumeController: false,
      loading: true,
      hasPendingAssistant: true,
      hasRunningAssistant: true,
      hydrateIfCold: true
    }),
    'watch'
  );
});

test('active session realtime recovery hydrates a hot session with no pending bubble', () => {
  assert.equal(
    resolveActiveSessionRealtimeRecoveryPlan({
      targetSessionId: 'session_demo',
      activeSessionId: 'session_demo',
      hasWatchController: false,
      hasSendController: false,
      hasResumeController: false,
      loading: true,
      hasPendingAssistant: false,
      hasRunningAssistant: false,
      hydrateIfCold: true
    }),
    'hydrate_then_watch'
  );
});

test('active session realtime recovery skips an idle warm cached session', () => {
  assert.equal(
    resolveActiveSessionRealtimeRecoveryPlan({
      targetSessionId: 'session_demo',
      activeSessionId: 'session_demo',
      hasWatchController: false,
      hasSendController: false,
      hasResumeController: false,
      loading: false,
      hasPendingAssistant: false,
      hasRunningAssistant: false,
      hydrateIfCold: true,
      hasWarmDetail: true,
      hasCachedMessages: true
    }),
    'skip_idle_session'
  );
});

test('active session realtime recovery skips an idle cold cached session', () => {
  assert.equal(
    resolveActiveSessionRealtimeRecoveryPlan({
      targetSessionId: 'session_demo',
      activeSessionId: 'session_demo',
      hasWatchController: false,
      hasSendController: false,
      hasResumeController: false,
      loading: false,
      hasPendingAssistant: false,
      hasRunningAssistant: false,
      hydrateIfCold: true,
      hasWarmDetail: false,
      hasCachedMessages: true
    }),
    'skip_idle_session'
  );
});

test('active session realtime recovery does not race an interactive send stream', () => {
  assert.equal(
    resolveActiveSessionRealtimeRecoveryPlan({
      targetSessionId: 'session_demo',
      activeSessionId: 'session_demo',
      hasWatchController: false,
      hasSendController: true,
      hasResumeController: false,
      loading: true,
      hasPendingAssistant: true,
      hasRunningAssistant: true,
      hydrateIfCold: true
    }),
    'skip_interactive_stream'
  );
});

test('active session realtime recovery reconciles only stale interactive streams', () => {
  const now = Date.now();
  assert.equal(
    shouldReconcileInteractiveStream({
      sendController: {},
      sendStartedAt: now - INTERACTIVE_STREAM_RECONCILE_IDLE_MS - 1000,
      sendLastEventAt: now - INTERACTIVE_STREAM_RECONCILE_IDLE_MS - 1000
    }),
    true
  );
  assert.equal(
    shouldReconcileInteractiveStream({
      sendController: {},
      sendStartedAt: now - INTERACTIVE_STREAM_RECONCILE_IDLE_MS - 1000,
      sendLastEventAt: now - 1000
    }),
    false
  );
  assert.equal(
    shouldReconcileInteractiveStream({
      sendStartedAt: now - INTERACTIVE_STREAM_RECONCILE_IDLE_MS - 1000,
      sendLastEventAt: now - INTERACTIVE_STREAM_RECONCILE_IDLE_MS - 1000
    }),
    false
  );
});

test('active session realtime recovery does not restart an existing watcher', () => {
  assert.equal(
    resolveActiveSessionRealtimeRecoveryPlan({
      targetSessionId: 'session_demo',
      activeSessionId: 'session_demo',
      hasWatchController: true,
      hasSendController: false,
      hasResumeController: false,
      loading: true,
      hasPendingAssistant: true,
      hasRunningAssistant: true,
      hydrateIfCold: true
    }),
    'skip_watching'
  );
});

test('session hydration does not restart a watcher for an idle thread', () => {
  assert.equal(
    shouldStartWatcherAfterSessionHydration({
      remoteRunning: false,
      runtimeStatus: 'idle',
      hasWatchController: false,
      hasSendController: false,
      hasResumeController: false
    }),
    false
  );
});

test('session hydration restarts a watcher for a running thread', () => {
  assert.equal(
    shouldStartWatcherAfterSessionHydration({
      remoteRunning: true,
      runtimeStatus: 'running',
      hasWatchController: false,
      hasSendController: false,
      hasResumeController: false
    }),
    true
  );
});

test('agent settings panel visibility is scoped to the agents section only', () => {
  assert.equal(shouldShowAgentSettingsPanelForSection('agents'), true);
  assert.equal(shouldShowAgentSettingsPanelForSection('tools'), false);
  assert.equal(shouldShowAgentSettingsPanelForSection('files'), false);
  assert.equal(shouldShowAgentSettingsPanelForSection('more'), false);
  assert.equal(shouldShowAgentSettingsPanelForSection('messages'), false);
});
