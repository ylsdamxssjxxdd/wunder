import test from 'node:test';
import assert from 'node:assert/strict';

import {
  captureChatSnapshotScheduleContext,
  resolveChatSnapshotScheduleSource
} from '../../src/stores/chatSnapshotScheduler';

test('keeps delayed snapshot bound to the session that scheduled it after switching foreground session', () => {
  const previousMessages = [{ id: 'b-1', role: 'assistant', content: 'previous agent preview' }];
  const context = captureChatSnapshotScheduleContext({
    activeSessionId: 'session-b',
    messages: previousMessages
  });
  const source = resolveChatSnapshotScheduleSource(
    {
      activeSessionId: 'session-a',
      messages: [{ id: 'a-1', role: 'assistant', content: 'current agent preview' }]
    },
    context,
    () => null
  );
  assert.deepEqual(source, {
    sessionId: 'session-b',
    messages: previousMessages
  });
});

test('uses the latest foreground messages when the scheduled snapshot still belongs to the active session', () => {
  const context = captureChatSnapshotScheduleContext({
    activeSessionId: 'session-a',
    messages: [{ id: 'a-old', role: 'assistant', content: 'stale' }]
  });
  const latestMessages = [{ id: 'a-new', role: 'assistant', content: 'fresh' }];
  const source = resolveChatSnapshotScheduleSource(
    {
      activeSessionId: 'session-a',
      messages: latestMessages
    },
    context,
    () => [{ id: 'cached', role: 'assistant', content: 'cached' }]
  );
  assert.deepEqual(source, {
    sessionId: 'session-a',
    messages: latestMessages
  });
});

test('prefers cached background session messages over the original scheduled array when available', () => {
  const context = captureChatSnapshotScheduleContext({
    activeSessionId: 'session-b',
    messages: [{ id: 'b-old', role: 'assistant', content: 'old' }]
  });
  const cachedMessages = [{ id: 'b-new', role: 'assistant', content: 'new' }];
  const source = resolveChatSnapshotScheduleSource(
    {
      activeSessionId: 'session-a',
      messages: [{ id: 'a-1', role: 'assistant', content: 'active' }]
    },
    context,
    () => cachedMessages
  );
  assert.deepEqual(source, {
    sessionId: 'session-b',
    messages: cachedMessages
  });
});
