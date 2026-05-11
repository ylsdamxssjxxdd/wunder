import test from 'node:test';
import assert from 'node:assert/strict';

import { resolveRuntimeDerivedStatus } from '../../src/stores/chatRuntimeDerivedStatus';

test('runtime derived status keeps running while a watcher owns a running thread', () => {
  const runtime = {
    threadStatus: 'running',
    loaded: true,
    activeTurnId: '',
    watchController: new AbortController(),
    watchActiveRoundCount: 1,
    sendController: null,
    resumeController: null,
    compactController: null,
    waitingForUserInput: false,
    pendingApprovalCount: 0,
    streamLifecycle: 'watching'
  };

  assert.equal(resolveRuntimeDerivedStatus({ runtime, loading: false }), 'running');
  assert.equal(runtime.loaded, true);
});

test('runtime derived status can settle to idle when no running controller remains', () => {
  const runtime = {
    threadStatus: 'running',
    loaded: true,
    activeTurnId: '',
    watchController: null,
    watchActiveRoundCount: 0,
    sendController: null,
    resumeController: null,
    compactController: null,
    waitingForUserInput: false,
    pendingApprovalCount: 0,
    streamLifecycle: 'idle'
  };

  assert.equal(resolveRuntimeDerivedStatus({ runtime, loading: false }), 'idle');
});

test('runtime derived status does not keep running for an idle watcher with no active rounds', () => {
  const runtime = {
    threadStatus: 'running',
    loaded: true,
    activeTurnId: '',
    watchController: new AbortController(),
    watchActiveRoundCount: 0,
    sendController: null,
    resumeController: null,
    compactController: null,
    waitingForUserInput: false,
    pendingApprovalCount: 0,
    streamLifecycle: 'watching'
  };

  assert.equal(resolveRuntimeDerivedStatus({ runtime, loading: false }), 'idle');
});

test('runtime derived status keeps running while watcher owns an active turn before round hydration', () => {
  const runtime = {
    threadStatus: 'running',
    loaded: true,
    activeTurnId: 'turn_demo',
    watchController: new AbortController(),
    watchActiveRoundCount: 0,
    sendController: null,
    resumeController: null,
    compactController: null,
    waitingForUserInput: false,
    pendingApprovalCount: 0,
    streamLifecycle: 'watching'
  };

  assert.equal(resolveRuntimeDerivedStatus({ runtime, loading: false }), 'running');
});
