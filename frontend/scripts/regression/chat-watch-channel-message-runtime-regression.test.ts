import test from 'node:test';
import assert from 'node:assert/strict';

import { consumeChatWatchChannelMessage } from '../../src/stores/chatWatchChannelMessageRuntime';

const normalizeEventId = (value: unknown): number | null => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    return null;
  }
  return Math.max(0, Math.trunc(parsed));
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

test('watch channel runtime appends user sideband messages with event ids', () => {
  const messages: Record<string, any>[] = [];
  let snapshotNotified = 0;

  const result = consumeChatWatchChannelMessage({
    messages,
    lastEventId: 10,
    eventId: 12,
    eventTimestampMs: Date.parse('2026-03-28T10:00:00.000Z'),
    payload: { role: 'user', content: 'hello' },
    data: { role: 'user', content: 'hello' },
    normalizeEventId,
    buildMessage,
    assignStreamEventId: (message, eventId) => {
      message.stream_event_id = eventId;
    },
    insertWatchUserMessage: () => {
      throw new Error('unexpected insert');
    },
    clearSupersededPendingAssistantMessages: () => {},
    dismissStaleInquiryPanels: () => {},
    touchUpdatedAt: () => {},
    notifySnapshot: () => {
      snapshotNotified += 1;
    },
    hiddenInternalUser: true,
    dedupeAssistantWindowMs: 2000
  });

  assert.deepEqual(result, { handled: true, lastEventId: 12, mutated: true });
  assert.equal(messages.length, 1);
  assert.equal(messages[0].role, 'user');
  assert.equal(messages[0].hiddenInternal, true);
  assert.equal(messages[0].stream_event_id, 12);
  assert.equal(snapshotNotified, 1);
});

test('watch channel runtime inserts user sideband before a pending assistant shell', () => {
  const messages: Record<string, any>[] = [
    { role: 'user', content: 'old question' },
    { role: 'assistant', content: 'old answer' },
    {
      role: 'assistant',
      content: '',
      stream_incomplete: true,
      workflowStreaming: true,
      reasoningStreaming: true
    }
  ];

  const result = consumeChatWatchChannelMessage({
    messages,
    lastEventId: 20,
    eventId: 21,
    eventTimestampMs: Date.parse('2026-04-01T10:00:00.000Z'),
    payload: { role: 'user', content: 'latest question' },
    data: { role: 'user', content: 'latest question' },
    normalizeEventId,
    buildMessage,
    assignStreamEventId: (message, eventId) => {
      message.stream_event_id = eventId;
    },
    insertWatchUserMessage: () => {
      throw new Error('unexpected insert');
    },
    clearSupersededPendingAssistantMessages: () => {},
    dismissStaleInquiryPanels: () => {},
    touchUpdatedAt: () => {},
    notifySnapshot: () => {},
    dedupeAssistantWindowMs: 2000
  });

  assert.deepEqual(result, { handled: true, lastEventId: 21, mutated: true });
  assert.equal(messages.length, 4);
  assert.deepEqual(
    messages.map((message) => message.role),
    ['user', 'assistant', 'user', 'assistant']
  );
  assert.equal(messages[2].content, 'latest question');
  assert.equal(messages[2].stream_event_id, 21);
  assert.equal(messages[3].stream_incomplete, true);
});

test('watch channel runtime reinserts late user sideband before the latest assistant when no pending shell remains', () => {
  const messages: Record<string, any>[] = [
    { role: 'user', content: 'old question', created_at: '2026-04-30T02:11:58.000Z' },
    { role: 'assistant', content: 'old answer', created_at: '2026-04-30T02:11:59.000Z' },
    {
      role: 'assistant',
      content: '50,000',
      created_at: '2026-04-30T02:12:05.000Z',
      stream_event_id: 80,
      stream_incomplete: false,
      workflowStreaming: false,
      reasoningStreaming: false
    }
  ];
  let insertedAnchorRole = '';
  const result = consumeChatWatchChannelMessage({
    messages,
    lastEventId: 80,
    eventId: 81,
    eventTimestampMs: Date.parse('2026-04-30T02:12:03.000Z'),
    payload: { role: 'user', content: '有多少人？' },
    data: { role: 'user', content: '有多少人？' },
    normalizeEventId,
    buildMessage,
    assignStreamEventId: (message, eventId) => {
      message.stream_event_id = eventId;
    },
    insertWatchUserMessage: (content, timestampMs, anchor, options = {}) => {
      insertedAnchorRole = String(anchor?.role || '');
      const userMessage: Record<string, any> = buildMessage('user', content, new Date(Number(timestampMs)).toISOString(), {
        hiddenInternal: Boolean((options as Record<string, unknown>).hiddenInternal)
      });
      userMessage.stream_event_id = 81;
      const anchorIndex = anchor ? messages.indexOf(anchor) : -1;
      if (anchorIndex >= 0) {
        messages.splice(anchorIndex, 0, userMessage);
      } else {
        messages.push(userMessage);
      }
    },
    clearSupersededPendingAssistantMessages: () => {},
    dismissStaleInquiryPanels: () => {},
    touchUpdatedAt: () => {},
    notifySnapshot: () => {},
    dedupeAssistantWindowMs: 2000
  });

  assert.deepEqual(result, { handled: true, lastEventId: 81, mutated: true });
  assert.equal(insertedAnchorRole, 'assistant');
  assert.deepEqual(
    messages.map((message) => `${message.role}:${message.content}`),
    ['user:old question', 'assistant:old answer', 'user:有多少人？', 'assistant:50,000']
  );
  assert.equal(messages[2].stream_event_id, 81);
});

test('watch channel runtime dedupes repeated assistant messages without event ids', () => {
  const messages = [
    {
      role: 'assistant',
      content: 'same reply',
      created_at: '2026-03-28T10:00:00.000Z'
    }
  ];

  const result = consumeChatWatchChannelMessage({
    messages,
    lastEventId: 2,
    eventId: null,
    eventTimestampMs: Date.parse('2026-03-28T10:00:01.000Z'),
    payload: { role: 'assistant', content: 'same reply' },
    data: { role: 'assistant', content: 'same reply' },
    normalizeEventId,
    buildMessage,
    assignStreamEventId: () => {},
    insertWatchUserMessage: () => {},
    clearSupersededPendingAssistantMessages: () => {},
    dismissStaleInquiryPanels: () => {},
    touchUpdatedAt: () => {},
    notifySnapshot: () => {},
    dedupeAssistantWindowMs: 2000
  });

  assert.deepEqual(result, { handled: true, lastEventId: 2, mutated: false });
  assert.equal(messages.length, 1);
});

test('watch channel runtime ignores duplicate assistant event ids', () => {
  const messages = [{ role: 'assistant', content: 'done', stream_event_id: 4 }];

  const result = consumeChatWatchChannelMessage({
    messages,
    lastEventId: 3,
    eventId: 4,
    eventTimestampMs: Date.now(),
    payload: { role: 'assistant', content: 'other' },
    data: { role: 'assistant', content: 'other' },
    normalizeEventId,
    buildMessage,
    assignStreamEventId: () => {},
    insertWatchUserMessage: () => {},
    clearSupersededPendingAssistantMessages: () => {},
    dismissStaleInquiryPanels: () => {},
    touchUpdatedAt: () => {},
    notifySnapshot: () => {},
    dedupeAssistantWindowMs: 2000
  });

  assert.deepEqual(result, { handled: true, lastEventId: 4, mutated: false });
  assert.equal(messages.length, 1);
});

test('watch channel runtime backfills event ids onto existing user history messages', () => {
  const messages: Record<string, any>[] = [
    {
      role: 'user',
      content: 'hello',
      created_at: '2026-04-04T00:14:06.826Z'
    },
    {
      role: 'assistant',
      content: 'hi',
      created_at: '2026-04-04T00:14:09.056Z'
    }
  ];

  const result = consumeChatWatchChannelMessage({
    messages,
    lastEventId: 90,
    eventId: 91,
    eventTimestampMs: Date.parse('2026-04-04T00:14:06.826Z'),
    payload: { role: 'user', content: 'hello' },
    data: { role: 'user', content: 'hello' },
    normalizeEventId,
    buildMessage,
    assignStreamEventId: (message, eventId) => {
      message.stream_event_id = eventId;
    },
    insertWatchUserMessage: () => {
      throw new Error('unexpected insert');
    },
    clearSupersededPendingAssistantMessages: () => {},
    dismissStaleInquiryPanels: () => {},
    touchUpdatedAt: () => {},
    notifySnapshot: () => {},
    dedupeAssistantWindowMs: 2000
  });

  assert.deepEqual(result, { handled: true, lastEventId: 91, mutated: false });
  assert.equal(messages.length, 2);
  assert.equal(messages[0].stream_event_id, 91);
});

test('watch channel runtime settles a pending assistant shell when assistant sideband arrives', () => {
  const messages: Record<string, any>[] = [
    { role: 'user', content: 'hello' },
    {
      role: 'assistant',
      content: '',
      stream_incomplete: true,
      workflowStreaming: true,
      reasoningStreaming: true
    }
  ];
  let snapshotNotified = 0;

  const result = consumeChatWatchChannelMessage({
    messages,
    lastEventId: 8,
    eventId: 9,
    eventTimestampMs: Date.parse('2026-03-28T10:00:02.000Z'),
    payload: { role: 'assistant', content: 'final reply' },
    data: { role: 'assistant', content: 'final reply' },
    normalizeEventId,
    buildMessage,
    assignStreamEventId: (message, eventId) => {
      message.stream_event_id = eventId;
    },
    insertWatchUserMessage: () => {},
    clearSupersededPendingAssistantMessages: () => {},
    dismissStaleInquiryPanels: () => {},
    touchUpdatedAt: () => {},
    notifySnapshot: () => {
      snapshotNotified += 1;
    },
    dedupeAssistantWindowMs: 2000
  });

  assert.deepEqual(result, { handled: true, lastEventId: 9, mutated: true });
  assert.equal(messages.length, 2);
  assert.equal(messages[1].content, 'final reply');
  assert.equal(messages[1].stream_event_id, 9);
  assert.equal(messages[1].stream_incomplete, false);
  assert.equal(messages[1].workflowStreaming, false);
  assert.equal(messages[1].reasoningStreaming, false);
  assert.equal(snapshotNotified, 1);
});

test('channel assistant message replaces stale pending assistant content from previous turn', () => {
  const messages = [
    {
      role: 'user',
      content: 'second question',
      created_at: '2026-04-02T10:00:00.000Z'
    },
    {
      role: 'assistant',
      content: 'this is the first round long answer that should not leak into the next turn',
      created_at: '2026-04-02T10:00:01.000Z',
      stream_incomplete: true,
      workflowStreaming: true,
      reasoningStreaming: false,
      stream_event_id: 11
    }
  ];
  let snapshotCalls = 0;
  let touchedAt = 0;

  const result = consumeChatWatchChannelMessage({
    messages,
    lastEventId: 11,
    eventId: 12,
    eventTimestampMs: Date.parse('2026-04-02T10:00:02.000Z'),
    payload: {
      role: 'assistant',
      content: 'short second answer'
    },
    data: {
      role: 'assistant',
      content: 'short second answer'
    },
    normalizeEventId,
    buildMessage,
    assignStreamEventId: (message, eventId) => {
      message.stream_event_id = eventId;
    },
    insertWatchUserMessage: () => {
      throw new Error('unexpected user insertion');
    },
    clearSupersededPendingAssistantMessages: () => undefined,
    dismissStaleInquiryPanels: () => undefined,
    touchUpdatedAt: (timestamp) => {
      touchedAt = Number(timestamp);
    },
    notifySnapshot: () => {
      snapshotCalls += 1;
    },
    dedupeAssistantWindowMs: 2000
  });

  assert.equal(result.handled, true);
  assert.equal(result.mutated, true);
  assert.equal(result.lastEventId, 12);
  assert.equal(messages.length, 2);
  assert.equal(messages[1].content, 'short second answer');
  assert.equal(messages[1].stream_event_id, 12);
  assert.equal(messages[1].stream_incomplete, false);
  assert.equal(messages[1].workflowStreaming, false);
  assert.equal(snapshotCalls, 1);
  assert.equal(touchedAt, Date.parse('2026-04-02T10:00:02.000Z'));
});
