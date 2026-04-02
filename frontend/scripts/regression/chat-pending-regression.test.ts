import test from 'node:test';
import assert from 'node:assert/strict';

import {
  clearSupersededPendingAssistantMessages,
  findPendingAssistantMessage,
  stopPendingAssistantMessage
} from '../../src/stores/chatPendingMessage';
import { isSessionBusyFromSignals } from '../../src/utils/chatSessionRuntime';
import { isCompactionRunningFromWorkflowItems, resolveLatestCompactionSnapshot } from '../../src/utils/chatCompactionWorkflow';

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

test('stops an assistant that only retains workflow and reasoning streaming flags', () => {
  const pending = {
    role: 'assistant',
    stream_incomplete: false,
    workflowStreaming: true,
    reasoningStreaming: true
  };
  assert.equal(stopPendingAssistantMessage(pending), true);
  assert.equal(pending.stream_incomplete, false);
  assert.equal(pending.workflowStreaming, false);
  assert.equal(pending.reasoningStreaming, false);
});

test('session busy remains true when compaction progress is still running', () => {
  const messages = [
    {
      role: 'assistant',
      stream_incomplete: false,
      workflowStreaming: false,
      reasoningStreaming: false,
      workflowItems: [{ eventType: 'compaction_progress', status: 'loading' }]
    }
  ];
  assert.equal(isSessionBusyFromSignals(false, messages), true);
});

test('session busy clears after cancelled compaction is finalized', () => {
  const messages = [
    {
      role: 'assistant',
      stream_incomplete: false,
      workflowStreaming: false,
      reasoningStreaming: false,
      workflowItems: [
        { eventType: 'compaction_progress', status: 'completed', detail: '{"status":"cancelled"}' },
        { eventType: 'compaction', status: 'completed', detail: '{"status":"cancelled"}' }
      ]
    }
  ];
  assert.equal(isSessionBusyFromSignals(false, messages), false);
});

test('session busy ignores stale running assistant markers from earlier turns', () => {
  const messages = [
    { role: 'user', content: 'first' },
    {
      role: 'assistant',
      stream_incomplete: true,
      workflowStreaming: true,
      reasoningStreaming: true,
      content: 'stale running'
    },
    { role: 'user', content: 'second' },
    {
      role: 'assistant',
      stream_incomplete: false,
      workflowStreaming: false,
      reasoningStreaming: false,
      content: 'done'
    }
  ];
  assert.equal(isSessionBusyFromSignals(false, messages), false);
});

test('compaction running detection prefers detail status when item status is stale', () => {
  const items = [{ eventType: 'compaction', status: 'completed', detail: '{"status":"loading"}' }];
  const snapshot = resolveLatestCompactionSnapshot(items);
  assert.equal(snapshot?.status, 'loading');
  assert.equal(isCompactionRunningFromWorkflowItems(items), true);
});

test('compaction progress without explicit status is treated as running', () => {
  const items = [{ eventType: 'compaction_progress' }];
  const snapshot = resolveLatestCompactionSnapshot(items);
  assert.equal(snapshot?.eventType, 'compaction_progress');
  assert.equal(snapshot?.explicitStatus, false);
  assert.equal(isCompactionRunningFromWorkflowItems(items), true);
});

test('compaction progress with explicit completed status is not treated as running', () => {
  const items = [{ eventType: 'compaction_progress', status: 'completed' }];
  const snapshot = resolveLatestCompactionSnapshot(items);
  assert.equal(snapshot?.status, 'completed');
  assert.equal(snapshot?.explicitStatus, true);
  assert.equal(isCompactionRunningFromWorkflowItems(items), false);
});
