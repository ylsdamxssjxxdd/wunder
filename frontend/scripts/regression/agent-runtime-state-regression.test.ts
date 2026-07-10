import test from 'node:test';
import assert from 'node:assert/strict';

import {
  hasAgentTerminalSettlementEvidence,
  isTerminalMessengerRuntimeStatus,
  isWaitingMessengerRuntimeStatus,
  resolveAgentRuntimeTerminalStateFromSessionStatus,
  resolveAgentRuntimeStateFromSignals,
  shouldSettleAgentRuntimeFromTerminalSession,
  shouldSettleAgentSessionsFromRuntimeState
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

test('agent runtime settlement only clears stale hot or terminal sessions', () => {
  assert.equal(
    shouldSettleAgentSessionsFromRuntimeState({
      previousState: 'running',
      nextState: 'idle'
    }),
    true
  );
  assert.equal(
    shouldSettleAgentSessionsFromRuntimeState({
      previousState: 'done',
      nextState: 'idle'
    }),
    true
  );
  assert.equal(
    shouldSettleAgentSessionsFromRuntimeState({
      previousState: 'idle',
      nextState: 'idle'
    }),
    false
  );
  assert.equal(
    shouldSettleAgentSessionsFromRuntimeState({
      previousState: 'idle',
      nextState: 'done'
    }),
    true
  );
});

test('agent runtime terminal settlement accepts active stale running as evidence', () => {
  assert.equal(
    hasAgentTerminalSettlementEvidence({
      targetSessionId: 'sess_active_terminal',
      activeSessionId: 'sess_active_terminal',
      currentState: 'running'
    }),
    true
  );
  assert.equal(
    hasAgentTerminalSettlementEvidence({
      targetSessionId: 'sess_active_terminal',
      currentRuntimeSessionId: 'sess_active_terminal',
      overrideState: 'running'
    }),
    true
  );
  assert.equal(
    hasAgentTerminalSettlementEvidence({
      targetSessionId: 'sess_old_terminal',
      activeSessionId: 'sess_other',
      currentRuntimeSessionId: 'sess_new',
      currentState: 'running'
    }),
    false
  );
});

test('agent runtime terminal session can settle stale running agent state', () => {
  assert.equal(resolveAgentRuntimeTerminalStateFromSessionStatus('idle'), 'done');
  assert.equal(resolveAgentRuntimeTerminalStateFromSessionStatus('not_loaded'), 'done');
  assert.equal(resolveAgentRuntimeTerminalStateFromSessionStatus('system_error'), 'error');
  assert.equal(resolveAgentRuntimeTerminalStateFromSessionStatus('running'), null);
  assert.equal(
    shouldSettleAgentRuntimeFromTerminalSession({
      sessionStatus: 'idle',
      currentState: 'running'
    }),
    true
  );
  assert.equal(
    shouldSettleAgentRuntimeFromTerminalSession({
      sessionStatus: 'not_loaded',
      currentState: 'idle',
      localStreaming: true
    }),
    true
  );
  assert.equal(
    shouldSettleAgentRuntimeFromTerminalSession({
      sessionStatus: 'idle',
      currentState: 'idle'
    }),
    false
  );
});
