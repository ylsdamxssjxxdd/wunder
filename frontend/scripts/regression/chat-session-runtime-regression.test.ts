import test from 'node:test';
import assert from 'node:assert/strict';

import {
  didThreadRuntimeEnterBusyState,
  hasActiveSubagentsAfterLatestUser,
  hasRunningAssistantMessage,
  hasStreamingAssistantMessage,
  isThreadRuntimeBusy,
  normalizeThreadRuntimeStatus
} from '../../src/utils/chatSessionRuntime';

const ensureBrowserRuntimeStub = (): void => {
  const root = globalThis as typeof globalThis & {
    localStorage?: {
      getItem: (key: string) => string | null;
      setItem: (key: string, value: string) => void;
      removeItem: (key: string) => void;
      clear: () => void;
    };
  };
  if (root.localStorage) return;
  const values = new Map<string, string>();
  root.localStorage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
    removeItem: (key: string) => {
      values.delete(key);
    },
    clear: () => {
      values.clear();
    }
  };
};

test('thread runtime entering running from idle requires reconcile fallback', () => {
  assert.equal(didThreadRuntimeEnterBusyState('idle', 'running'), true);
  assert.equal(didThreadRuntimeEnterBusyState('not_loaded', 'running'), true);
});

test('thread runtime entering waiting state from non-busy requires reconcile fallback', () => {
  assert.equal(didThreadRuntimeEnterBusyState('idle', 'waiting_approval'), true);
  assert.equal(didThreadRuntimeEnterBusyState('system_error', 'waiting_user_input'), true);
});

test('thread runtime transitions within busy states do not retrigger reconcile fallback', () => {
  assert.equal(didThreadRuntimeEnterBusyState('running', 'waiting_approval'), false);
  assert.equal(didThreadRuntimeEnterBusyState('waiting_approval', 'waiting_user_input'), false);
  assert.equal(didThreadRuntimeEnterBusyState('running', 'running'), false);
});

test('thread runtime leaving or staying outside busy state does not trigger reconcile fallback', () => {
  assert.equal(didThreadRuntimeEnterBusyState('running', 'idle'), false);
  assert.equal(didThreadRuntimeEnterBusyState('idle', 'idle'), false);
  assert.equal(didThreadRuntimeEnterBusyState('not_loaded', 'system_error'), false);
});

test('thread runtime queued state is recoverable without masquerading as running', async () => {
  ensureBrowserRuntimeStub();
  const { isTerminalRuntimeStatus } = await import('../../src/stores/chatWorkflowHydration');
  assert.equal(normalizeThreadRuntimeStatus('queued'), 'queued');
  assert.equal(isThreadRuntimeBusy('queued'), false);
  assert.equal(isTerminalRuntimeStatus('queued'), false);
  assert.equal(didThreadRuntimeEnterBusyState('idle', 'queued'), false);
});

test('active subagents keep a thread busy without reopening parent stream state', () => {
  const messages = [
    { role: 'user', content: 'delegate work' },
    {
      role: 'assistant',
      workflowStreaming: false,
      stream_incomplete: false,
      subagents: [
        {
          session_id: 'child-session',
          run_id: 'child-run',
          status: 'timeout',
          terminal: 'false',
          failed: 'false'
        }
      ]
    }
  ];

  assert.equal(hasActiveSubagentsAfterLatestUser(messages), true);
  assert.equal(hasStreamingAssistantMessage(messages), false);
  assert.equal(hasRunningAssistantMessage(messages), true);
});

test('assistant waiting for first output keeps the thread busy before streaming flags arrive', () => {
  const startMs = Date.UTC(2026, 3, 30, 9, 0, 0);
  const messages = [
    { role: 'user', content: 'hello' },
    {
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
    }
  ];

  assert.equal(hasStreamingAssistantMessage(messages), true);
  assert.equal(hasRunningAssistantMessage(messages), true);
});
