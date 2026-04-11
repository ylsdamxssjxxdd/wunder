import test from 'node:test';
import assert from 'node:assert/strict';

import {
  hasAssistantPendingQuestion,
  isAssistantMessageRunning,
  normalizeAssistantMessageRuntimeState,
  resolveAssistantMessageRuntimeState
} from '../../src/utils/assistantMessageRuntime';

test('assistant runtime treats pending question as higher priority than running flags', () => {
  const message = {
    role: 'assistant',
    stream_incomplete: true,
    questionPanel: {
      status: 'pending'
    },
    state: 'running'
  };

  assert.equal(hasAssistantPendingQuestion(message), true);
  assert.equal(resolveAssistantMessageRuntimeState(message), 'pending');
});

test('assistant runtime treats compaction progress as a running message', () => {
  const message = {
    role: 'assistant',
    workflowItems: [
      {
        eventType: 'compaction_progress',
        status: 'loading'
      }
    ]
  };

  assert.equal(isAssistantMessageRunning(message), true);
  assert.equal(resolveAssistantMessageRuntimeState(message), 'running');
});

test('assistant runtime falls back to done for completed assistant bubbles without explicit state', () => {
  assert.equal(resolveAssistantMessageRuntimeState({ role: 'assistant', content: 'finished' }), 'done');
});

test('assistant runtime normalizes terminal aliases consistently', () => {
  assert.equal(normalizeAssistantMessageRuntimeState('completed'), 'done');
  assert.equal(normalizeAssistantMessageRuntimeState('cancelled'), 'error');
  assert.equal(normalizeAssistantMessageRuntimeState('awaiting_confirmation'), 'pending');
});
