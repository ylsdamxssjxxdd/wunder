import test from 'node:test';
import assert from 'node:assert/strict';

import {
  shouldKeepForegroundInteractiveRuntime,
  shouldKeepForegroundLiveMessagesDuringRunningGap
} from '../../src/stores/chatWatchLifecycle';

test('foreground running gap keeps live messages when remote runtime is still running and hydrated snapshot lost the pending assistant', () => {
  assert.equal(
    shouldKeepForegroundLiveMessagesDuringRunningGap({
      preserveWatcher: true,
      lifecycle: 'watching',
      hasSendController: false,
      hasResumeController: false,
      remoteRunning: true,
      liveHasPendingAssistant: true,
      hydratedHasPendingAssistant: false
    }),
    true
  );
});

test('foreground running gap does not keep live messages once hydrated snapshot also carries the pending assistant', () => {
  assert.equal(
    shouldKeepForegroundLiveMessagesDuringRunningGap({
      preserveWatcher: true,
      lifecycle: 'watching',
      hasSendController: false,
      hasResumeController: false,
      remoteRunning: true,
      liveHasPendingAssistant: true,
      hydratedHasPendingAssistant: true
    }),
    false
  );
});

test('foreground running gap does not keep live messages when remote runtime is already idle', () => {
  assert.equal(
    shouldKeepForegroundLiveMessagesDuringRunningGap({
      preserveWatcher: true,
      lifecycle: 'watching',
      hasSendController: false,
      hasResumeController: false,
      remoteRunning: false,
      liveHasPendingAssistant: true,
      hydratedHasPendingAssistant: false
    }),
    false
  );
});

test('interactive runtime stays active while remote runtime still reports running', () => {
  assert.equal(
    shouldKeepForegroundInteractiveRuntime({
      remoteRunning: true,
      hasSendController: false,
      hasResumeController: false
    }),
    true
  );
});

test('interactive runtime stays active while a local send controller is still alive', () => {
  assert.equal(
    shouldKeepForegroundInteractiveRuntime({
      remoteRunning: false,
      hasSendController: true,
      hasResumeController: false
    }),
    true
  );
});

test('interactive runtime can settle only after both remote runtime and local interactive controllers are idle', () => {
  assert.equal(
    shouldKeepForegroundInteractiveRuntime({
      remoteRunning: false,
      hasSendController: false,
      hasResumeController: false
    }),
    false
  );
});
