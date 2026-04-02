import test from 'node:test';
import assert from 'node:assert/strict';

import { didThreadRuntimeEnterBusyState } from '../../src/utils/chatSessionRuntime';

test('thread runtime entering running from idle requires reconcile fallback', () => {
  assert.equal(didThreadRuntimeEnterBusyState('idle', 'running'), true);
  assert.equal(didThreadRuntimeEnterBusyState('not_loaded', 'running'), true);
});

test('thread runtime entering waiting state from non-busy requires reconcile fallback', () => {
  assert.equal(didThreadRuntimeEnterBusyState('idle', 'waiting_approval'), true);
  assert.equal(didThreadRuntimeEnterBusyState('system_error', 'waiting_user_input'), true);
});

test('thread runtime transitions within busy states do not retrigger reconcile fallback', () => {
  assert.equal(didThreadRuntimeEnterBusyState('running', 'waiting_approval'), false);
  assert.equal(didThreadRuntimeEnterBusyState('waiting_approval', 'waiting_user_input'), false);
  assert.equal(didThreadRuntimeEnterBusyState('running', 'running'), false);
});

test('thread runtime leaving or staying outside busy state does not trigger reconcile fallback', () => {
  assert.equal(didThreadRuntimeEnterBusyState('running', 'idle'), false);
  assert.equal(didThreadRuntimeEnterBusyState('idle', 'idle'), false);
  assert.equal(didThreadRuntimeEnterBusyState('not_loaded', 'system_error'), false);
});
