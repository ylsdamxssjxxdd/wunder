import test from 'node:test';
import assert from 'node:assert/strict';

import { resolveActiveSessionRealtimeRecoveryPlan } from '../../src/stores/chatActiveSessionRealtime';
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

test('agent settings panel visibility is scoped to the agents section only', () => {
  assert.equal(shouldShowAgentSettingsPanelForSection('agents'), true);
  assert.equal(shouldShowAgentSettingsPanelForSection('tools'), false);
  assert.equal(shouldShowAgentSettingsPanelForSection('files'), false);
  assert.equal(shouldShowAgentSettingsPanelForSection('more'), false);
  assert.equal(shouldShowAgentSettingsPanelForSection('messages'), false);
});
