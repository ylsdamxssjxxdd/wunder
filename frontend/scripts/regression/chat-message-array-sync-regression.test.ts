import test from 'node:test';
import assert from 'node:assert/strict';

import {
  replaceMessageArrayKeepingReference,
  resolveRealtimeMessageArrayReference
} from '../../src/stores/chatMessageArraySync';

test('replaceMessageArrayKeepingReference preserves the original array object', () => {
  const target = [{ role: 'user', content: 'old' }];
  const next = [
    { role: 'user', content: 'new user' },
    { role: 'assistant', content: 'new reply' }
  ];

  const result = replaceMessageArrayKeepingReference(target, next);

  assert.equal(result, target);
  assert.deepEqual(target, next);
});

test('replaceMessageArrayKeepingReference returns the next array when no target exists', () => {
  const next = [{ role: 'assistant', content: 'reply' }];
  const result = replaceMessageArrayKeepingReference(null, next);

  assert.equal(result, next);
});

test('resolveRealtimeMessageArrayReference prefers the active foreground array', () => {
  const activeMessages = [{ role: 'assistant', content: 'foreground' }];
  const cachedMessages = [{ role: 'assistant', content: 'cached' }];

  const result = resolveRealtimeMessageArrayReference({
    sessionId: 'sess-1',
    activeSessionId: 'sess-1',
    activeMessages,
    cachedMessages
  });

  assert.equal(result, activeMessages);
});

test('resolveRealtimeMessageArrayReference never reuses active foreground messages for another session', () => {
  const activeMessages = [{ role: 'assistant', content: 'foreground' }];
  const fallbackMessages = [{ role: 'assistant', content: 'fallback' }];

  const result = resolveRealtimeMessageArrayReference({
    sessionId: 'sess-2',
    activeSessionId: 'sess-1',
    activeMessages,
    cachedMessages: null,
    fallbackMessages
  });

  assert.equal(result, fallbackMessages);
});

test('replaceMessageArrayKeepingReference can rebind a watcher onto the current foreground array', () => {
  const staleWatched = [{ role: 'assistant', content: 'streaming live' }];
  const foreground = [{ role: 'assistant', content: 'hydrated stale' }];

  const rebound = replaceMessageArrayKeepingReference(foreground, staleWatched);

  assert.equal(rebound, foreground);
  assert.deepEqual(foreground, staleWatched);
  staleWatched.push({ role: 'assistant', content: 'new stale item' });
  assert.equal(foreground.length, 1);
});
