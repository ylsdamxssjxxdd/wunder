import test from 'node:test';
import assert from 'node:assert/strict';
import { isReactive, reactive } from 'vue';
import { createPinia, setActivePinia } from 'pinia';
import { buildMessageVirtualWindow } from '../../src/views/messenger/messageVirtualWindow';
import { useMessageMarkdownCache } from '../../src/components/chat/messageMarkdownCache';
import { isChatSnapshotCurrent } from '../../src/stores/chatSnapshotFreshness';
import { selectVisibleMessageProjections } from '../../src/realtime/chat/chatRuntimeSelectors';
import { isTerminalLlmOutputPayload } from '../../src/utils/chatStreamTerminal';

const values = new Map<string, string>();
Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: {
  getItem: (key: string) => values.get(key) ?? null,
  setItem: (key: string, value: string) => values.set(key, value),
  removeItem: (key: string) => values.delete(key), clear: () => values.clear()
} });

const setup = async () => {
  setActivePinia(createPinia());
  const { useChatStore } = await import('../../src/stores/chat');
  const store = useChatStore();
  store.resetState();
  store.activeSessionId = 'session-1';
  store.sessions = [{ id: 'session-1', agent_id: '' }];
  return store;
};

test('stream transport cleanup cannot settle the canonical running thread', async () => {
  const store = await setup();
  const { applyCanonicalStreamRuntimeEvent, ensureRuntime } = await import('../../src/stores/chatRuntimeState');
  const { setSessionLoading } = await import('../../src/stores/chatRuntimeControls');
  applyCanonicalStreamRuntimeEvent(store, 'session-1', 'thread_status', { status: 'running' }, '1');
  setSessionLoading(store, 'session-1', false);
  assert.deepEqual([store.isSessionBusy('session-1'), ensureRuntime('session-1').threadStatus], [true, 'running']);
  assert.equal(isReactive(store.runtimeProjection.sessions['session-1']), false);
  applyCanonicalStreamRuntimeEvent(store, 'session-1', 'thread_status', { status: 'idle' }, '2');
  assert.equal(store.isSessionBusy('session-1'), false);
  store.resetState();
});

test('runtime snapshot can change status at the same persisted event cursor', async () => {
  const store = await setup();
  const { applyCanonicalSessionEventsSnapshot } = await import('../../src/stores/chatRuntimeState');
  for (const [status, expected] of [['running', true], ['idle', false], ['running', true]] as const) {
    applyCanonicalSessionEventsSnapshot(store, 'session-1', {
      last_event_id: 10, runtime: { status }, running: expected
    });
    assert.equal(store.isSessionBusy('session-1'), expected);
  }
  store.resetState();
});

test('overlapping snapshots reject the older request even without an intervening delta', async () => {
  const store = await setup();
  const { default: api } = await import('../../src/api/http');
  const { loadSessionEventsSnapshot, applyCanonicalSessionEventsSnapshot } =
    await import('../../src/stores/chatRuntimeState');
  const original = api.defaults.adapter;
  const pending: Array<(status: string) => void> = [];
  api.defaults.adapter = config => new Promise(resolve => pending.push(status => resolve({
    status: 200, statusText: 'OK', headers: {}, config,
    data: { data: { runtime: { status }, running: status === 'running', last_event_id: 10 } }
  })));
  try {
    const first = loadSessionEventsSnapshot('session-1', { allowCached: false, dedupeInFlight: false });
    const second = loadSessionEventsSnapshot('session-1', { allowCached: false, dedupeInFlight: false });
    await new Promise(resolve => setTimeout(resolve, 0));
    pending[1]('running');
    applyCanonicalSessionEventsSnapshot(store, 'session-1', await second);
    pending[0]('idle');
    applyCanonicalSessionEventsSnapshot(store, 'session-1', await first);
    assert.equal(store.isSessionBusy('session-1'), true);
  } finally { api.defaults.adapter = original; store.resetState(); }
});

test('older idle response cannot overwrite a newer stream or optimistic send', async () => {
  const store = await setup();
  const { applyCanonicalStreamRuntimeEvent, applyCanonicalSessionEventsSnapshot,
    applyCanonicalClientMessageSubmittedRuntimeEvent, ensureRuntime } = await import('../../src/stores/chatRuntimeState');
  const oldResponse = { __clientRuntimeRevision: 0, last_event_id: 10,
    runtime: { status: 'idle' }, running: false };
  applyCanonicalClientMessageSubmittedRuntimeEvent(store, {
    sessionId: 'session-1', content: 'input', clientMessageId: 'client-1'
  });
  assert.equal(isChatSnapshotCurrent(ensureRuntime('session-1'), oldResponse), false);
  applyCanonicalStreamRuntimeEvent(store, 'session-1', 'llm_output_delta', {
    user_round: 1, model_round: 1, delta: 'partial', event_seq: 11
  }, '11');
  applyCanonicalSessionEventsSnapshot(store, 'session-1', oldResponse);
  assert.equal(store.isSessionBusy('session-1'), true);
  store.resetState();
});

test('snapshot normalization preserves canonical identity, round order and cancellation', async () => {
  const { normalizeSnapshotMessage, buildChatSnapshot, writeChatSnapshot, readChatSnapshot } =
    await import('../../src/stores/chatSnapshot');
  const input = { role: 'assistant', content: 'output', created_at: '', message_id: 'history:2',
    user_turn_id: 'user-turn:session-1:round:2', model_turn_id: 'model-turn:session-1:user:2:model:1',
    history_id: 2, turn_index: 4, user_turn_index: 2, model_turn_index: 1,
    user_round: 2, model_round: 1, status: 'cancelled', cancelled: true };
  const normalized = normalizeSnapshotMessage(input);
  for (const [key, value] of Object.entries(input)) assert.deepEqual(normalized[key], value);
  writeChatSnapshot(buildChatSnapshot('session-1', [input]));
  assert.equal(readChatSnapshot()?.messages[0].message_id, input.message_id);
});

test('virtual scroll reuses height prefix but remeasures after layout changes', () => {
  let reads = 0;
  let height = 100;
  const options = { items: Array.from({ length: 2000 }, (_, index) => ({ key: String(index) })),
    enabled: true, scrollTop: 0, viewportHeight: 600, overscan: 4, tailPinCount: 4,
    estimatedHeight: 100, layoutVersion: 0, resolveHeight: () => { reads++; return height; } };
  buildMessageVirtualWindow(options);
  for (let index = 0; index < 100; index++) buildMessageVirtualWindow({ ...options, scrollTop: index * 100 });
  assert.equal(reads, 2000);
  height = 200;
  const changed = buildMessageVirtualWindow({ ...options, layoutVersion: 1 });
  assert.deepEqual([reads, changed.totalHeight], [4000, 400000]);
});

test('markdown cache survives virtual rows and remains bounded and isolated', () => {
  const owner = reactive({});
  const first = useMessageMarkdownCache(owner);
  first.writeMarkdownCacheEntry('row', 'source', '<p>source</p>');
  assert.equal(useMessageMarkdownCache(owner).readMarkdownCacheEntry('row')?.html, '<p>source</p>');
  assert.equal(useMessageMarkdownCache({}).readMarkdownCacheEntry('row'), null);
  for (let index = 0; index < 100; index++) first.writeMarkdownCacheEntry(String(index), 'x', '<p>x</p>');
  assert.equal(first.readMarkdownCacheEntry('row'), null);
});

const transcript = () => [
  { role: 'user', content: 'input-1', message_id: 'history:1', turn_index: 1,
    user_turn_id: 'user-turn:session-1:round:1' },
  { role: 'assistant', content: 'output-1', message_id: 'history:2', turn_index: 2,
    user_turn_id: 'user-turn:session-1:round:1', model_turn_id: 'model-turn:session-1:user:1:model:1' },
  { role: 'user', content: 'input-2', message_id: 'history:3', turn_index: 3,
    user_turn_id: 'user-turn:session-1:round:2' }
];

test('repeated running transcript hydration preserves the deduplicated server tail and order', async () => {
  const store = await setup();
  const { syncChatRuntimeProjectionFromSnapshot, applyCanonicalStreamRuntimeEvent } =
    await import('../../src/stores/chatRuntimeState');
  syncChatRuntimeProjectionFromSnapshot(store, 'session-1', transcript(), { running: true });
  applyCanonicalStreamRuntimeEvent(store, 'session-1', 'llm_output_delta', {
    user_round: 2, model_round: 1, delta: 'partial-2'
  }, '11');
  const visible = () => selectVisibleMessageProjections(store.runtimeProjection, 'session-1')
    .map(message => [message.role, message.content]);
  const expected = [['user', 'input-1'], ['assistant', 'output-1'], ['user', 'input-2'], ['assistant', 'partial-2']];
  assert.deepEqual(visible(), expected);
  for (let index = 0; index < 3; index++) {
    syncChatRuntimeProjectionFromSnapshot(store, 'session-1', transcript(), { running: true });
    // A repeated event cannot be relied upon to reconstruct a deleted row.
    applyCanonicalStreamRuntimeEvent(store, 'session-1', 'llm_output_delta', {
      user_round: 2, model_round: 1, delta: 'partial-2'
    }, '11');
    assert.deepEqual(visible(), expected);
    assert.equal(store.isSessionBusy('session-1'), true);
  }
  syncChatRuntimeProjectionFromSnapshot(store, 'session-1', [
    ...transcript(), { role: 'assistant', content: 'answer-2', message_id: 'history:4',
      turn_index: 4, user_turn_id: 'user-turn:session-1:round:2',
      model_turn_id: 'model-turn:session-1:user:2:model:1' }
  ], { running: false, loading: false, authoritative: true });
  assert.deepEqual(visible(), [...expected.slice(0, 3), ['assistant', 'answer-2']]);
  assert.equal(store.isSessionBusy('session-1'), false);
  store.resetState();
});

test('detail response racing live output seeds history without rolling back content or status', async () => {
  const store = await setup();
  const { default: api } = await import('../../src/api/http');
  const { applyCanonicalStreamRuntimeEvent } = await import('../../src/stores/chatRuntimeState');
  const originalAdapter = api.defaults.adapter;
  const pending: Array<() => void> = [];
  api.defaults.adapter = async (config) => new Promise(resolve => {
    pending.push(() => resolve({ status: 200, statusText: 'OK', headers: {}, config, data: { data:
      config.url?.endsWith('/events')
        ? { running: false, runtime: { status: 'idle' }, last_event_id: 10, events: [] }
        : { id: 'session-1', transcript: transcript() }
    } }));
  });
  try {
    const loading = store.loadSessionDetail('session-1', { startWatcherAfterHydration: false });
    await new Promise(resolve => setTimeout(resolve, 0));
    assert.equal(pending.length, 2);
    applyCanonicalStreamRuntimeEvent(store, 'session-1', 'llm_output_delta', {
      user_round: 2, model_round: 1, delta: 'new-output'
    }, '11');
    pending.forEach(resolve => resolve());
    await loading;
    assert.deepEqual(selectVisibleMessageProjections(store.runtimeProjection, 'session-1')
      .map(message => [message.role, message.content]),
    [['user', 'input-1'], ['assistant', 'output-1'], ['user', 'input-2'], ['assistant', 'new-output']]);
    assert.equal(store.isSessionBusy('session-1'), true);
  } finally {
    api.defaults.adapter = originalAdapter;
    store.resetState();
  }
});

test('model tool-call completion never ends the request or clears its busy state', async () => {
  const store = await setup();
  const { applyCanonicalStreamRuntimeEvent } = await import('../../src/stores/chatRuntimeState');
  for (const reason of ['tool_calls', 'function_call', 'tool_use']) {
    const payload = { user_round: 1, model_round: 1, content: 'partial', finish_reason: reason };
    assert.equal(isTerminalLlmOutputPayload(payload), false);
    applyCanonicalStreamRuntimeEvent(store, 'session-1', 'llm_output', payload, reason);
    assert.equal(store.isSessionBusy('session-1'), true);
  }
  assert.equal(isTerminalLlmOutputPayload({ done: true, tool_calls: [{}] }), false);
  assert.equal(isTerminalLlmOutputPayload({ stop_reason: 'final_tool' }), true);
  store.resetState();
});

test('replaced resume cleanup cannot cancel the turn or clear the new controller', async () => {
  const store = await setup();
  const { chatWsClient } = await import('../../src/stores/chatWatcher');
  const { ensureRuntime, applyCanonicalStreamRuntimeEvent } = await import('../../src/stores/chatRuntimeState');
  const request = chatWsClient.request;
  const pending: Array<{ resolve: () => void; reject: (error: Error) => void }> = [];
  chatWsClient.request = () => new Promise<void>((resolve, reject) => pending.push({ resolve, reject }));
  // Suppress a post-resume watcher; the two requests themselves are real store actions.
  store.activeSessionId = null;
  try {
    const first = store.resumeStream('session-1', null, { force: true });
    const second = store.resumeStream('session-1', null, { force: true });
    const controller = ensureRuntime('session-1').resumeController;
    pending[0].reject(Object.assign(new Error('replaced'), { name: 'AbortError' }));
    await first;
    assert.equal(ensureRuntime('session-1').resumeController, controller);
    assert.equal(store.isSessionBusy('session-1'), true);
    applyCanonicalStreamRuntimeEvent(store, 'session-1', 'thread_status', { status: 'idle' }, '1');
    pending[1].resolve();
    await second;
    assert.equal(store.isSessionBusy('session-1'), false);
  } finally { chatWsClient.request = request; store.resetState(); }
});

test('stale idle event cannot clear approval state or settle a newer running turn', async () => {
  const store = await setup();
  const { ensureRuntime, applyCanonicalStreamRuntimeEvent, applySessionRuntimeEvent } =
    await import('../../src/stores/chatRuntimeState');
  applyCanonicalStreamRuntimeEvent(store, 'session-1', 'thread_status', { status: 'running' }, '3');
  const runtime = ensureRuntime('session-1');
  runtime.pendingApprovalIds = ['approval-1'];
  runtime.pendingApprovalCount = 1;
  applyCanonicalStreamRuntimeEvent(store, 'session-1', 'thread_status', { status: 'idle' }, '2');
  applySessionRuntimeEvent(store, 'session-1', { status: 'idle' });
  assert.deepEqual([store.isSessionBusy('session-1'), runtime.pendingApprovalIds], [true, ['approval-1']]);
  store.resetState();
});
