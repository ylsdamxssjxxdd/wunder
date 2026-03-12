import test from 'node:test';
import assert from 'node:assert/strict';

import {
  clearSupersededPendingAssistantMessages,
  findPendingAssistantMessage,
  stopPendingAssistantMessage
} from '../../src/stores/chatPendingMessage';

test('finds the latest trailing pending assistant message', () => {
  const pending = { role: 'assistant', stream_incomplete: true, content: 'working' };
  const messages = [
    { role: 'user', content: 'first' },
    { role: 'assistant', stream_incomplete: false, content: 'done' },
    { role: 'user', content: 'second' },
    pending
  ];
  assert.equal(findPendingAssistantMessage(messages), pending);
});

test('ignores superseded pending assistant messages once a newer user turn exists', () => {
  const stalePending = { role: 'assistant', stream_incomplete: true, content: 'old pending' };
  const messages = [
    { role: 'user', content: 'first' },
    stalePending,
    { role: 'user', content: 'continue' }
  ];
  assert.equal(findPendingAssistantMessage(messages), null);
});

test('clears only superseded pending assistant messages', () => {
  const stalePending = {
    role: 'assistant',
    stream_incomplete: true,
    workflowStreaming: true,
    reasoningStreaming: true
  };
  const activePending = {
    role: 'assistant',
    stream_incomplete: true,
    workflowStreaming: true,
    reasoningStreaming: true
  };
  const messages = [
    { role: 'user', content: 'first' },
    stalePending,
    { role: 'user', content: 'second' },
    activePending
  ];
  assert.equal(clearSupersededPendingAssistantMessages(messages), true);
  assert.equal(stalePending.stream_incomplete, false);
  assert.equal(stalePending.workflowStreaming, false);
  assert.equal(stalePending.reasoningStreaming, false);
  assert.equal(activePending.stream_incomplete, true);
});

test('stops an active pending assistant in place', () => {
  const pending = {
    role: 'assistant',
    stream_incomplete: true,
    workflowStreaming: true,
    reasoningStreaming: true
  };
  assert.equal(stopPendingAssistantMessage(pending), true);
  assert.equal(pending.stream_incomplete, false);
  assert.equal(pending.workflowStreaming, false);
  assert.equal(pending.reasoningStreaming, false);
});
