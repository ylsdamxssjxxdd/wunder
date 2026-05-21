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

const frameQueue: Array<FrameRequestCallback> = [];
let frameId = 0;
const timerQueue: Array<() => void> = [];

Object.assign(globalThis, {
  window: {
    __WUNDER_DESKTOP_RUNTIME__: null,
    location: {
      origin: 'http://localhost'
    }
  },
  localStorage: localStorageMock,
  fetch: async () => ({
    ok: false,
    json: async () => ({})
  }),
  requestAnimationFrame: (callback: FrameRequestCallback) => {
    frameQueue.push(callback);
    frameId += 1;
    return frameId;
  },
  cancelAnimationFrame: () => undefined,
  setTimeout: (callback: () => void) => {
    timerQueue.push(callback);
    return timerQueue.length;
  },
  clearTimeout: () => undefined
});

const { createWorkflowProcessor } = require('../../src/stores/chatWorkflowProcessor');
const { notifySessionSnapshot } = require('../../src/stores/chatRuntimeState');

const runNextFrame = () => {
  const callback = frameQueue.shift();
  if (callback) {
    callback(0);
  }
};

const runNextTimer = () => {
  const callback = timerQueue.shift();
  if (callback) {
    callback();
  }
};

const createAssistantMessage = () => ({
  role: 'assistant',
  content: '',
  created_at: '2026-05-10T00:00:00.000Z',
  workflowItems: [],
  workflowStreaming: true,
  stream_incomplete: true,
  stream_round: 1,
  stats: {
    contextTokens: null,
    contextPreviewTokens: null,
    contextTotalTokens: null
  }
});

test('workflow processor batches streaming deltas before mutating visible bubble content', () => {
  frameQueue.length = 0;
  timerQueue.length = 0;
  const assistantMessage = createAssistantMessage();
  let snapshotCount = 0;
  const processor = createWorkflowProcessor(
    assistantMessage,
    { globalRound: 0, currentRound: null },
    () => {
      snapshotCount += 1;
    },
    {
      streamFlushMs: 40,
      finalizeWithNow: false,
      commandSessionStore: {
        upsertSnapshot: () => null,
        appendDelta: () => null
      }
    }
  );

  let visibleContentWrites = 0;
  let visibleContent = assistantMessage.content;
  Object.defineProperty(assistantMessage, 'content', {
    get: () => visibleContent,
    set: (value) => {
      visibleContentWrites += 1;
      visibleContent = String(value ?? '');
    },
    configurable: true
  });

  for (let index = 0; index < 20; index += 1) {
    processor.handleEvent(
      'llm_output_delta',
      JSON.stringify({
        delta: String(index % 10),
        model_round: 1
      })
    );
  }

  assert.equal(assistantMessage.content, '');
  assert.equal(visibleContentWrites, 0);
  assert.equal(snapshotCount, 0);
  assert.equal(timerQueue.length, 1);
  assert.equal(frameQueue.length, 0);

  runNextTimer();
  assert.equal(assistantMessage.content, '');
  assert.equal(visibleContentWrites, 0);
  assert.equal(snapshotCount, 0);
  assert.equal(frameQueue.length, 1);

  runNextFrame();
  assert.equal(assistantMessage.content, '01234567890123456789');
  assert.equal(visibleContentWrites, 1);
  assert.equal(snapshotCount, 1);
});

test('workflow processor force flushes pending stream text on final output', () => {
  frameQueue.length = 0;
  timerQueue.length = 0;
  const assistantMessage = createAssistantMessage();
  const processor = createWorkflowProcessor(
    assistantMessage,
    { globalRound: 0, currentRound: null },
    null,
    {
      streamFlushMs: 80,
      finalizeWithNow: false,
      commandSessionStore: {
        upsertSnapshot: () => null,
        appendDelta: () => null
      }
    }
  );

  processor.handleEvent(
    'llm_output_delta',
    JSON.stringify({
      delta: 'partial',
      model_round: 1
    })
  );
  assert.equal(assistantMessage.content, '');
  assert.equal(timerQueue.length, 1);

  processor.handleEvent(
    'llm_output',
    JSON.stringify({
      content: 'final answer',
      model_round: 1
    })
  );

  assert.equal(assistantMessage.content, 'final answer');
  assert.equal(frameQueue.length, 0);
});

test('session snapshot version ignores invisible stream bookkeeping changes', () => {
  frameQueue.length = 0;
  timerQueue.length = 0;
  const messages = [
    {
      role: 'user',
      content: 'hello',
      created_at: '2026-05-10T00:00:00.000Z'
    },
    createAssistantMessage()
  ];
  const store = {
    activeSessionId: 'session-a',
    messages,
    messageMutationVersion: 0,
    runtimeProjectionVersion: 0,
    runtimeProjection: null,
    loadingBySession: {},
    sessions: [{ id: 'session-a', agent_id: 'agent-a' }]
  };

  notifySessionSnapshot(store, 'session-a', messages, true);
  assert.equal(store.messageMutationVersion, 1);

  notifySessionSnapshot(store, 'session-a', messages, false);
  assert.equal(store.messageMutationVersion, 1);

  messages[1].content = 'visible';
  notifySessionSnapshot(store, 'session-a', messages, false);
  assert.equal(store.messageMutationVersion, 2);
});
