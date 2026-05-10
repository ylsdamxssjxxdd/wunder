import test from 'node:test';
import assert from 'node:assert/strict';

const storage = new Map<string, string>();
const localStorageMock = {
  getItem: (key: string) => storage.get(key) ?? null,
  setItem: (key: string, value: string) => {
    storage.set(key, String(value));
  },
  removeItem: (key: string) => {
    storage.delete(key);
  }
};

const windowMock = {
  __WUNDER_DESKTOP_RUNTIME__: null,
  location: {
    origin: 'http://localhost'
  }
};

Object.assign(globalThis, {
  window: windowMock,
  localStorage: localStorageMock,
  fetch: async () => ({
    ok: false,
    json: async () => ({})
  })
});

const { createWorkflowProcessor } = require('../../src/stores/chatWorkflowProcessor');
const { buildAssistantMessageStatsEntries } = require('../../src/utils/messageStats');

const t = (key: string): string => {
  const table: Record<string, string> = {
    'chat.stats.duration': 'Duration',
    'chat.stats.speed': 'Speed',
    'chat.stats.contextTokens': 'Context',
    'chat.stats.quota': 'Quota',
    'chat.stats.toolCalls': 'Tools',
    'messenger.messageStatus.done': 'Done'
  };
  return table[key] || key;
};

const findEntryValue = (
  entries: Array<{ label: string; value: string }>,
  label: string
): string | null => entries.find((item) => item.label === label)?.value ?? null;

test('first assistant bubble keeps observed occupancy from final workflow event', () => {
  const assistantMessage = {
    role: 'assistant',
    content: '',
    created_at: '2026-05-10T00:00:00.000Z',
    workflowItems: [],
    workflowStreaming: false,
    stream_incomplete: false,
    stream_round: 1,
    stats: {
      contextTokens: null,
      contextPreviewTokens: null,
      contextTotalTokens: null
    }
  };
  let snapshotCount = 0;
  let observedSessionContext: number | null = null;
  const processor = createWorkflowProcessor(
    assistantMessage,
    { globalRound: 0, currentRound: null },
    () => {
      snapshotCount += 1;
    },
    {
      finalizeWithNow: false,
      commandSessionStore: {
        upsertSnapshot: () => null,
        appendDelta: () => null
      },
      onContextUsage: (contextTokens: number) => {
        observedSessionContext = contextTokens;
      }
    }
  );

  processor.handleEvent(
    'context_usage',
    JSON.stringify({
      context_tokens: 0,
      context_usage_source: 'unobserved',
      user_round: 1
    })
  );
  processor.handleEvent(
    'final',
    JSON.stringify({
      answer: 'done',
      usage: {
        input_tokens: 1000,
        output_tokens: 25,
        total_tokens: 1025
      },
      round_usage: {
        input_tokens: 1000,
        output_tokens: 25,
        total_tokens: 1025
      },
      context_occupancy_tokens: 1025,
      request_consumed_tokens: 1025,
      user_round: 1
    })
  );
  processor.finalize();

  const messages = [{ role: 'user', content: 'first turn' }, assistantMessage];
  const entries = buildAssistantMessageStatsEntries(assistantMessage, t, messages);

  assert.equal(findEntryValue(entries, 'Context'), '1025');
  assert.equal(findEntryValue(entries, 'Quota'), '1025');
  assert.equal(observedSessionContext, 1025);
  assert.ok(snapshotCount > 0);
});
