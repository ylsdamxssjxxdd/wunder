import test from 'node:test';
import assert from 'node:assert/strict';

import {
  captureStopRunSnapshot,
  validateStopRunSnapshot
} from '../../src/views/messenger/stopRunGuard';

const userMessage = (content: string, createdAt: string) => ({
  role: 'user',
  content,
  created_at: createdAt
});

const runningAssistant = (createdAt: string, streamRound = 1, content = '') => ({
  role: 'assistant',
  content,
  created_at: createdAt,
  stream_round: streamRound,
  waiting_updated_at_ms: Date.parse(createdAt),
  workflowStreaming: true,
  stream_incomplete: true
});

const idleAssistant = (createdAt: string, streamRound = 1, content = 'done') => ({
  role: 'assistant',
  content,
  created_at: createdAt,
  stream_round: streamRound,
  workflowStreaming: false,
  stream_incomplete: false
});

test('stop run guard rejects a delayed confirmation after the run completed', () => {
  const expected = captureStopRunSnapshot({
    sessionId: 'sess_a',
    busy: true,
    messages: [
      userMessage('first', '2026-05-10T00:00:00.000Z'),
      runningAssistant('2026-05-10T00:00:01.000Z', 1)
    ]
  });
  const current = captureStopRunSnapshot({
    sessionId: 'sess_a',
    busy: false,
    messages: [
      userMessage('first', '2026-05-10T00:00:00.000Z'),
      idleAssistant('2026-05-10T00:00:01.000Z', 1)
    ]
  });

  assert.deepEqual(validateStopRunSnapshot(expected, current, 'sess_a'), {
    ok: false,
    reason: 'not_running'
  });
});

test('stop run guard rejects a delayed confirmation after a new user turn starts', () => {
  const expected = captureStopRunSnapshot({
    sessionId: 'sess_a',
    busy: true,
    messages: [
      userMessage('first', '2026-05-10T00:00:00.000Z'),
      runningAssistant('2026-05-10T00:00:01.000Z', 1)
    ]
  });
  const current = captureStopRunSnapshot({
    sessionId: 'sess_a',
    busy: true,
    messages: [
      userMessage('first', '2026-05-10T00:00:00.000Z'),
      idleAssistant('2026-05-10T00:00:01.000Z', 1),
      userMessage('second', '2026-05-10T00:00:03.000Z'),
      runningAssistant('2026-05-10T00:00:04.000Z', 2)
    ]
  });

  assert.deepEqual(validateStopRunSnapshot(expected, current, 'sess_a'), {
    ok: false,
    reason: 'run_changed'
  });
});

test('stop run guard rejects a delayed confirmation after active session changed', () => {
  const expected = captureStopRunSnapshot({
    sessionId: 'sess_a',
    busy: true,
    messages: [
      userMessage('first', '2026-05-10T00:00:00.000Z'),
      runningAssistant('2026-05-10T00:00:01.000Z', 1)
    ]
  });
  const current = captureStopRunSnapshot({
    sessionId: 'sess_a',
    busy: true,
    messages: [
      userMessage('first', '2026-05-10T00:00:00.000Z'),
      runningAssistant('2026-05-10T00:00:01.000Z', 1)
    ]
  });

  assert.deepEqual(validateStopRunSnapshot(expected, current, 'sess_b'), {
    ok: false,
    reason: 'session_changed'
  });
});

test('stop run guard allows the same streaming run as content grows', () => {
  const expected = captureStopRunSnapshot({
    sessionId: 'sess_a',
    busy: true,
    messages: [
      userMessage('first', '2026-05-10T00:00:00.000Z'),
      runningAssistant('2026-05-10T00:00:01.000Z', 1, '')
    ]
  });
  const current = captureStopRunSnapshot({
    sessionId: 'sess_a',
    busy: true,
    messages: [
      userMessage('first', '2026-05-10T00:00:00.000Z'),
      runningAssistant('2026-05-10T00:00:01.000Z', 1, 'partial output')
    ]
  });

  assert.deepEqual(validateStopRunSnapshot(expected, current, 'sess_a'), {
    ok: true,
    reason: null
  });
});

test('stop run guard allows a later assistant round under the same user turn', () => {
  const expected = captureStopRunSnapshot({
    sessionId: 'sess_a',
    busy: true,
    messages: [
      userMessage('first', '2026-05-10T00:00:00.000Z'),
      runningAssistant('2026-05-10T00:00:01.000Z', 1, 'using tool')
    ]
  });
  const current = captureStopRunSnapshot({
    sessionId: 'sess_a',
    busy: true,
    messages: [
      userMessage('first', '2026-05-10T00:00:00.000Z'),
      idleAssistant('2026-05-10T00:00:01.000Z', 1, 'tool done'),
      runningAssistant('2026-05-10T00:00:03.000Z', 2, 'continuing')
    ]
  });

  assert.deepEqual(validateStopRunSnapshot(expected, current, 'sess_a'), {
    ok: true,
    reason: null
  });
});
