import test from 'node:test';
import assert from 'node:assert/strict';

import {
  hasAssistantPendingQuestion,
  hasAssistantWaitingForCurrentOutput,
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

test('assistant runtime treats local waiting placeholder before first output as waiting current output', () => {
  const startMs = Date.UTC(2026, 3, 30, 9, 0, 0);
  const message = {
    role: 'assistant',
    workflowStreaming: false,
    stream_incomplete: false,
    waiting_updated_at_ms: startMs,
    waiting_first_output_at_ms: null,
    waiting_phase_first_output_at_ms: null,
    stats: {
      interaction_start_ms: startMs,
      interaction_end_ms: null
    }
  };

  assert.equal(isAssistantMessageRunning(message), false);
  assert.equal(hasAssistantWaitingForCurrentOutput(message), true);
  assert.equal(resolveAssistantMessageRuntimeState(message), 'done');
});

test('assistant runtime does not reopen failed terminal messages from stale waiting timestamps', () => {
  const startMs = Date.UTC(2026, 3, 30, 9, 0, 0);
  const endMs = startMs + 4000;
  const message = {
    role: 'assistant',
    waiting_updated_at_ms: startMs,
    waiting_first_output_at_ms: null,
    waiting_phase_first_output_at_ms: null,
    workflowItems: [
      {
        eventType: 'request_failed',
        status: 'failed'
      }
    ],
    stats: {
      interaction_start_ms: startMs,
      interaction_end_ms: endMs
    }
  };

  assert.equal(hasAssistantWaitingForCurrentOutput(message), false);
  assert.equal(resolveAssistantMessageRuntimeState(message), 'done');
});

test('assistant runtime normalizes terminal aliases consistently', () => {
  assert.equal(normalizeAssistantMessageRuntimeState('completed'), 'done');
  assert.equal(normalizeAssistantMessageRuntimeState('cancelled'), 'error');
  assert.equal(normalizeAssistantMessageRuntimeState('awaiting_confirmation'), 'pending');
});
