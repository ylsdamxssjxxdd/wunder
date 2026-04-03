import test from 'node:test';
import assert from 'node:assert/strict';

import {
  mergeProtectedRealtimeMessages,
  upsertProtectedRealtimeMessage
} from '../../src/stores/chatRealtimeMessageProtection';

const normalizeEventId = (value: unknown): number | null => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const buildMessage = (
  role: 'user' | 'assistant',
  content: string,
  createdAt?: string,
  meta: Record<string, unknown> = {}
) => ({
  role,
  content,
  created_at: createdAt,
  ...meta
});

test('protected realtime messages are restored when detail refresh misses the channel turn', () => {
  const entries = upsertProtectedRealtimeMessage(
    [],
    {
      eventId: 41,
      role: 'user',
      content: 'from channel',
      createdAt: '2026-04-03T09:00:00.000Z'
    },
    normalizeEventId
  );
  const messages: Record<string, any>[] = [
    { role: 'assistant', content: '', stream_incomplete: true, workflowStreaming: true }
  ];

  const result = mergeProtectedRealtimeMessages({
    messages,
    entries,
    normalizeEventId,
    buildMessage,
    assignStreamEventId: (message, eventId) => {
      message.stream_event_id = eventId;
    }
  });

  assert.equal(result.mutated, true);
  assert.equal(messages.length, 2);
  assert.deepEqual(
    messages.map((message) => message.role),
    ['user', 'assistant']
  );
  assert.equal(messages[0].stream_event_id, 41);
  assert.equal(messages[0].realtime_protected, true);
  assert.equal(result.retainedEntries.length, 1);
});

test('protected realtime entries are released once history includes the same stream event', () => {
  const entries = upsertProtectedRealtimeMessage(
    [],
    {
      eventId: 41,
      role: 'user',
      content: 'from channel',
      createdAt: '2026-04-03T09:00:00.000Z'
    },
    normalizeEventId
  );
  const messages: Record<string, any>[] = [
    { role: 'user', content: 'from channel', stream_event_id: 41 },
    { role: 'assistant', content: 'server reply', stream_event_id: 42 }
  ];

  const result = mergeProtectedRealtimeMessages({
    messages,
    entries,
    normalizeEventId,
    buildMessage,
    assignStreamEventId: (message, eventId) => {
      message.stream_event_id = eventId;
    }
  });

  assert.equal(result.mutated, false);
  assert.equal(result.retainedEntries.length, 0);
  assert.equal(messages.length, 2);
});

test('protected entries keep local copies that are already marked as realtime protected', () => {
  const entries = upsertProtectedRealtimeMessage(
    [],
    {
      eventId: 41,
      role: 'user',
      content: 'from channel',
      createdAt: '2026-04-03T09:00:00.000Z'
    },
    normalizeEventId
  );
  const messages: Record<string, any>[] = [
    {
      role: 'user',
      content: 'from channel',
      stream_event_id: 41,
      realtime_protected: true
    }
  ];

  const result = mergeProtectedRealtimeMessages({
    messages,
    entries,
    normalizeEventId,
    buildMessage,
    assignStreamEventId: (message, eventId) => {
      message.stream_event_id = eventId;
    }
  });

  assert.equal(result.mutated, false);
  assert.equal(result.retainedEntries.length, 1);
});
