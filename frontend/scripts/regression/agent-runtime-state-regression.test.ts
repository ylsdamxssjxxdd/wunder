import test from 'node:test';
import assert from 'node:assert/strict';

import {
  isTerminalMessengerRuntimeStatus,
  isWaitingMessengerRuntimeStatus,
  resolveAgentRuntimeStateFromSignals
} from '../../src/views/messenger/agentRuntimeState';

test('agent runtime state keeps queued/waiting ahead of stale local streaming', () => {
  assert.equal(isWaitingMessengerRuntimeStatus('queued'), true);
  assert.equal(
    resolveAgentRuntimeStateFromSignals({
      localWaiting: true,
      localStreaming: true,
      remoteState: 'idle'
    }),
    'pending'
  );
  assert.equal(
    resolveAgentRuntimeStateFromSignals({
      localStreaming: true,
      remoteState: 'pending'
    }),
    'pending'
  );
});

test('agent runtime state lets authoritative terminal beat stale running override', () => {
  assert.equal(isTerminalMessengerRuntimeStatus('completed'), true);
  assert.equal(
    resolveAgentRuntimeStateFromSignals({
      localStreaming: true,
      remoteState: 'done',
      overrideState: 'running'
    }),
    'done'
  );
  assert.equal(
    resolveAgentRuntimeStateFromSignals({
      localStreaming: true,
      remoteState: 'error',
      overrideState: 'running'
    }),
    'error'
  );
});

test('agent runtime state still uses local streaming as running fallback', () => {
  assert.equal(
    resolveAgentRuntimeStateFromSignals({
      localStreaming: true,
      remoteState: 'idle'
    }),
    'running'
  );
});
