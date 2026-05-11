import test from 'node:test';
import assert from 'node:assert/strict';

import {
  ALL_SESSION_LIST_CACHE_KEY,
  DEFAULT_AGENT_CACHE_KEY,
  isAllSessionsCacheKeyCollidingWithDefaultAgent,
  normalizeSessionListItems,
  resolveLoadSessionsCacheKey
} from '../../src/stores/chatSessionListLoadCache';

const normalizeAgentKey = (agentId: string): string => String(agentId || '').trim() || DEFAULT_AGENT_CACHE_KEY;

test('session list load cache keeps all-sessions key separate from default-agent cache', () => {
  assert.equal(resolveLoadSessionsCacheKey(null, normalizeAgentKey), ALL_SESSION_LIST_CACHE_KEY);
  assert.equal(resolveLoadSessionsCacheKey('', normalizeAgentKey), DEFAULT_AGENT_CACHE_KEY);
  assert.equal(resolveLoadSessionsCacheKey(DEFAULT_AGENT_CACHE_KEY, normalizeAgentKey), DEFAULT_AGENT_CACHE_KEY);
  assert.equal(isAllSessionsCacheKeyCollidingWithDefaultAgent(normalizeAgentKey), false);
});

test('session list load cache ignores malformed list entries before merging', () => {
  const session = { id: 'session_a', title: 'Session A' };
  const normalized = normalizeSessionListItems([
    session,
    null,
    '',
    ['bad'],
    { id: 'session_b', title: 'Session B' }
  ]);

  assert.deepEqual(normalized, [
    session,
    { id: 'session_b', title: 'Session B' }
  ]);
});
