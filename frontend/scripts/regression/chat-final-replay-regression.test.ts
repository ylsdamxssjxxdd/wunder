import test from 'node:test';
import assert from 'node:assert/strict';

import { mergeFinalTranscriptAssistantDuplicates } from '../../src/stores/chatFinalTranscriptMerge';
import { consumeChatWatchChannelMessage } from '../../src/stores/chatWatchChannelMessageRuntime';
import { mergeProtectedRealtimeMessages } from '../../src/stores/chatRealtimeMessageProtection';

const normalizeEventId = (value: unknown): number | null => {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? Math.trunc(parsed) : null;
};

const assignStreamEventId = (message: Record<string, any>, eventId: unknown): void => {
  const normalized = normalizeEventId(eventId);
  if (normalized !== null) {
    message.stream_event_id = normalized;
  }
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

const finalAnswer =
  'The final response is long enough to prove that a hydrated workflow bubble and a persisted transcript bubble represent one assistant output after replay.';

test('session hydration merges streamed final workflow bubble with persisted final transcript bubble', () => {
  const messages = [
    { role: 'user', content: 'request', created_at: '2026-05-01T10:00:00.000Z' },
    {
      role: 'assistant',
      content: finalAnswer,
      created_at: '2026-05-01T10:00:05.000Z',
      stream_round: 4,
      workflowItems: [
        { eventType: 'llm_request', status: 'completed', title: 'request' },
        { eventType: 'llm_output', status: 'completed', title: 'output' }
      ],
      stats: { output_tokens: 31 }
    },
    {
      role: 'assistant',
      content: finalAnswer,
      created_at: '2026-05-01T10:00:06.000Z',
      stream_round: 4
    }
  ];

  const merged = mergeFinalTranscriptAssistantDuplicates(messages);
  assert.equal(merged.length, 2);
  assert.equal(merged[1].content, finalAnswer);
  assert.equal(merged[1].workflowItems.length, 2);
  assert.deepEqual(merged[1].stats, { output_tokens: 31 });
});

test('assistant sideband final message backfills completed streaming bubble instead of appending a plain duplicate', () => {
  const messages: Record<string, any>[] = [
    { role: 'user', content: 'request' },
    {
      role: 'assistant',
      content: finalAnswer,
      stream_event_id: 41,
      stream_incomplete: false,
      workflowStreaming: false,
      reasoningStreaming: false,
      workflowItems: [{ eventType: 'final', status: 'completed' }]
    }
  ];

  const result = consumeChatWatchChannelMessage({
    messages,
    lastEventId: 41,
    eventId: 42,
    eventTimestampMs: Date.parse('2026-05-01T10:00:07.000Z'),
    payload: { role: 'assistant', content: finalAnswer },
    data: { role: 'assistant', content: finalAnswer },
    normalizeEventId,
    buildMessage,
    assignStreamEventId,
    insertWatchUserMessage: () => {
      throw new Error('unexpected user insertion');
    },
    clearSupersededPendingAssistantMessages: () => undefined,
    dismissStaleInquiryPanels: () => undefined,
    touchUpdatedAt: () => undefined,
    notifySnapshot: () => undefined,
    dedupeAssistantWindowMs: 2000
  });

  assert.equal(result.handled, true);
  assert.equal(result.mutated, false);
  assert.equal(result.lastEventId, 42);
  assert.equal(messages.length, 2);
  assert.equal(messages[1].stream_event_id, 42);
  assert.equal(messages[1].workflowItems.length, 1);
});

test('assistant sideband final message does not backfill a matching answer from an earlier user turn', () => {
  const messages: Record<string, any>[] = [
    { role: 'user', content: 'first request', created_at: '2026-05-01T10:00:00.000Z' },
    {
      role: 'assistant',
      content: finalAnswer,
      created_at: '2026-05-01T10:00:05.000Z',
      stream_event_id: 41,
      stream_incomplete: false,
      workflowStreaming: false,
      reasoningStreaming: false
    },
    { role: 'user', content: 'second request', created_at: '2026-05-01T10:05:00.000Z' }
  ];
  let snapshotCount = 0;

  const result = consumeChatWatchChannelMessage({
    messages,
    lastEventId: 41,
    eventId: 42,
    eventTimestampMs: Date.parse('2026-05-01T10:05:07.000Z'),
    payload: { role: 'assistant', content: finalAnswer },
    data: { role: 'assistant', content: finalAnswer },
    normalizeEventId,
    buildMessage,
    assignStreamEventId,
    insertWatchUserMessage: () => {
      throw new Error('unexpected user insertion');
    },
    clearSupersededPendingAssistantMessages: () => undefined,
    dismissStaleInquiryPanels: () => undefined,
    touchUpdatedAt: () => undefined,
    notifySnapshot: () => {
      snapshotCount += 1;
    },
    dedupeAssistantWindowMs: 2000
  });

  assert.equal(result.handled, true);
  assert.equal(result.mutated, true);
  assert.equal(result.lastEventId, 42);
  assert.equal(messages.length, 4);
  assert.equal(messages[1].stream_event_id, 41);
  assert.equal(messages[3].stream_event_id, 42);
  assert.equal(snapshotCount, 1);
});

test('protected realtime assistant final message matches existing completed output across event ids', () => {
  const messages: Record<string, any>[] = [
    { role: 'user', content: 'request' },
    {
      role: 'assistant',
      content: finalAnswer,
      created_at: '2026-05-01T10:00:05.000Z',
      stream_event_id: 80,
      stream_incomplete: false,
      workflowStreaming: false,
      reasoningStreaming: false,
      workflowItems: [{ eventType: 'llm_output', status: 'completed' }]
    }
  ];

  const result = mergeProtectedRealtimeMessages({
    messages,
    entries: [
      {
        eventId: 81,
        role: 'assistant',
        content: finalAnswer,
        createdAt: '2026-05-01T10:00:20.000Z',
        trackedAt: Date.parse('2026-05-01T10:00:20.000Z')
      }
    ],
    normalizeEventId,
    buildMessage,
    assignStreamEventId
  });

  assert.equal(result.mutated, true);
  assert.equal(result.retainedEntries.length, 0);
  assert.equal(messages.length, 2);
  assert.equal(messages[1].stream_event_id, 81);
  assert.equal(messages[1].workflowItems.length, 1);
});

test('protected realtime assistant final message does not match an earlier user turn', () => {
  const messages: Record<string, any>[] = [
    { role: 'user', content: 'first request' },
    {
      role: 'assistant',
      content: finalAnswer,
      created_at: '2026-05-01T10:00:05.000Z',
      stream_event_id: 80,
      stream_incomplete: false,
      workflowStreaming: false,
      reasoningStreaming: false
    },
    { role: 'user', content: 'second request' }
  ];

  const result = mergeProtectedRealtimeMessages({
    messages,
    entries: [
      {
        eventId: 81,
        role: 'assistant',
        content: finalAnswer,
        createdAt: '2026-05-01T10:05:20.000Z',
        trackedAt: Date.parse('2026-05-01T10:05:20.000Z')
      }
    ],
    normalizeEventId,
    buildMessage,
    assignStreamEventId
  });

  assert.equal(result.mutated, true);
  assert.equal(result.retainedEntries.length, 1);
  assert.equal(messages.length, 4);
  assert.equal(messages[1].stream_event_id, 80);
  assert.equal(messages[3].stream_event_id, 81);
  assert.equal(messages[3].realtime_protected, true);
});
