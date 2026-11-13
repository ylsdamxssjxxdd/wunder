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

test('explicitly idle session snapshot does not replay stale runtime controllers', async () => {
  const { shouldApplySessionEventsSnapshotToProjection } = await import('../../src/stores/chatRuntimeState');
  assert.equal(
    shouldApplySessionEventsSnapshotToProjection(
      { running: false, queued: false },
      { threadStatus: 'running', sendController: {} }
    ),
    false
  );
  assert.equal(
    shouldApplySessionEventsSnapshotToProjection(
      { running: true },
      { threadStatus: 'idle' }
    ),
    true
  );
});

test('idle hydration immediately clears a fresh manual compaction marker', async () => {
  const { clearCompletedAssistantStreamingState, syncChatRuntimeProjectionFromSnapshot } =
    await import('../../src/stores/chatRuntimeState');
  const store = await setup();
  const messages = [{ role: 'assistant', content: '[Context summary] retained',
    created_at: new Date().toISOString(), manual_compaction_marker: true,
    workflowStreaming: true, stream_incomplete: true, reasoningStreaming: true,
    workflowItems: [{ eventType: 'compaction_progress', status: 'loading',
      detail: JSON.stringify({ trigger_mode: 'manual', status: 'loading' }) }] }];
  clearCompletedAssistantStreamingState(messages);
  assert.deepEqual([messages[0].workflowStreaming, messages[0].stream_incomplete,
    messages[0].reasoningStreaming], [false, false, false]);
  syncChatRuntimeProjectionFromSnapshot(store, 'session-1', messages, {
    running: false, loading: false, authoritative: true
  });
  assert.deepEqual(selectVisibleMessageProjections(store.runtimeProjection, 'session-1')
    .map(message => [message.content, message.status]), [['[Context summary] retained', 'final']]);
  assert.equal(store.isSessionBusy('session-1'), false);
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
    created_seq: 2, turn_index: 4, user_turn_index: 2, model_turn_index: 1,
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

const viewResponse = (config: any, data: any) => ({
  status: 200, statusText: 'OK', headers: {}, config, data: { data }
});

const viewSession = (overrides: Record<string, any> = {}) => ({ id: 'session-1', agent_id: '', ...overrides });

const viewRow = (overrides: Record<string, any> = {}) => ({
  root_turn_id: 'turn-1', execution_turn_id: 'turn-1', user_round: 1, revision: 3,
  status: 'completed', user: { item_id: 'u1', content: 'input-1' },
  assistant: { item_id: 'a1', content: 'output-1' }, workflow_count: 0, failed_count: 0,
  ...overrides
});

const viewPage = (rows: any[], overrides: Record<string, any> = {}) => ({
  session_id: 'session-1', epoch: 1, view_seq: 8, projection_state: 'ready',
  thread_empty: rows.length === 0, latest_round: 1, chat: rows, trajectory: [], tiles: [],
  tile_level: 0, next_before: 1, has_more: false, runtime_status: 'idle', ...overrides
});

// 打开线程只走有界投影 bootstrap；事件尾与 workflow 轮次已不再是历史来源。
const mockThreadViewBootstrap = (
  first: { page: any; session: any },
  options: { onOpen?: (index: number) => Promise<any> | null } = {}
) => {
  let opens = 0;
  return async (config: any) => {
    const url = String(config.url || '');
    if (url.endsWith('/view/bootstrap')) {
      const index = opens++;
      const pending = options.onOpen?.(index);
      if (pending) return pending;
      return viewResponse(config, index === 0 ? first : { page: first.page, session: first.session });
    }
    return viewResponse(config, { events: [], rounds: [], running: false });
  };
};

test('an older bootstrap page cannot rewind a newer committed transcript', async () => {
  const { beginThreadView, commitThreadView, clearThreadView, threadViewHasNewerPage, threadViewSlots } =
    await import('../../src/realtime/chat/threadViewStore');
  const key = 'session-1';
  clearThreadView(key);
  const generation = beginThreadView(key);
  assert.equal(commitThreadView(key, viewPage([viewRow()], { view_seq: 8 }), generation), true);
  const stale = viewPage([viewRow({ revision: 1, assistant: { item_id: 'a1', content: 'stale-output' } })], { view_seq: 4 });
  // 旧页必须被游标守卫拒绝；打开路径靠 threadViewHasNewerPage 识别「更旧但合法」的页。
  assert.equal(commitThreadView(key, stale, generation), false);
  assert.equal(threadViewHasNewerPage(key, stale, generation), true);
  assert.deepEqual((threadViewSlots(key) ?? []).map(slot => [slot.assistant.content, slot.assistant.status]),
    [['output-1', 'completed']]);
  clearThreadView(key);
});

test('cold detail restores history and tool counts from the bootstrap page alone', async () => {
  const store = await setup();
  const { default: api } = await import('../../src/api/http');
  const { threadViewSlots } = await import('../../src/realtime/chat/threadViewStore');
  const original = api.defaults.adapter;
  api.defaults.adapter = mockThreadViewBootstrap({
    page: viewPage([viewRow({ workflow_count: 1, process_items: [{ item_id: 'tool-1', kind: 'tool',
      status: 'completed', payload: { tool: 'read_file', tool_call_id: 'call-1', ok: true, status: 'completed' } }] })]),
    session: viewSession({ runtime_status: 'completed' })
  });
  try {
    await store.loadSessionDetail('session-1', { startWatcherAfterHydration: false });
    const slots = threadViewSlots('session-1') ?? [];
    assert.equal(slots.length, 1);
    assert.equal(slots[0].assistant.content, 'output-1');
    assert.equal(slots[0].assistant.status, 'completed');
    assert.equal(slots[0].assistant.streaming, false);
    assert.equal(slots[0].assistant.process_count, 1);
    assert.deepEqual((slots[0].assistant.process_items ?? []).map(item => item.item_id), ['tool-1']);
    const assistants = store.messages.filter(message => message.role === 'assistant' && !message.isGreeting);
    assert.equal(assistants.length, 1);
    assert.equal(store.isSessionBusy('session-1'), false);
  } finally { api.defaults.adapter = original; store.resetState(); }
});

test('completed compaction row stays idle during detail hydration', async () => {
  const store = await setup();
  const { default: api } = await import('../../src/api/http');
  const { threadViewSlots } = await import('../../src/realtime/chat/threadViewStore');
  const original = api.defaults.adapter;
  api.defaults.adapter = mockThreadViewBootstrap({
    page: viewPage([viewRow({
      workflow_count: 1,
      assistant: { item_id: 'a1', content: '' },
      process_items: [{ item_id: 'compaction-1', kind: 'compaction', status: 'completed',
        payload: { trigger_mode: 'manual', status: 'completed', summary_text: '[Context summary] retained' } }]
    })], { runtime_status: 'idle' }),
    session: viewSession({ runtime_status: 'idle' })
  });
  try {
    await store.loadSessionDetail('session-1', { startWatcherAfterHydration: false });
    const slot = (threadViewSlots('session-1') ?? [])[0];
    assert.equal(slot?.assistant.status, 'completed');
    assert.equal(slot?.assistant.streaming, false);
    assert.equal(store.isSessionBusy('session-1'), false);
    assert.equal(store.messages.filter(message => message.role === 'assistant' && !message.isGreeting).length, 1);
  } finally { api.defaults.adapter = original; store.resetState(); }
});

test('preloading an inactive thread only prefetches and never commits a view', async () => {
  const store = await setup();
  const { default: api } = await import('../../src/api/http');
  const { threadViewSlots } = await import('../../src/realtime/chat/threadViewStore');
  const original = api.defaults.adapter;
  let bootstrapRequests = 0;
  api.defaults.adapter = async config => {
    if (String(config.url || '').endsWith('/view/bootstrap')) {
      bootstrapRequests += 1;
      return viewResponse(config, { page: viewPage([viewRow()]), session: viewSession() });
    }
    return viewResponse(config, { events: [], rounds: [], running: false });
  };
  try {
    store.activeSessionId = 'session-2';
    await store.preloadSessionDetail('session-1', { force: true, syncActive: false });
    assert.equal(bootstrapRequests, 1);
    // 预取只填缓存，不提交投影、不改变活动线程。
    assert.equal(threadViewSlots('session-1'), null);
    assert.equal(store.activeSessionId, 'session-2');
  } finally { api.defaults.adapter = original; store.resetState(); }
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

test('recovery reloads a snapshot without inventing a running turn or creating a second stream', async () => {
  const store = await setup();
  const { chatWsClient } = await import('../../src/stores/chatWatcher');
  const { ensureRuntime } = await import('../../src/stores/chatRuntimeState');
  const request = chatWsClient.request;
  const detail = store.loadSessionDetail;
  const pending: Array<() => void> = [];
  let streamRequests = 0;
  chatWsClient.request = () => { streamRequests++; return Promise.resolve(); };
  store.loadSessionDetail = () => new Promise<void>(resolve => pending.push(resolve));
  store.activeSessionId = null;
  try {
    const first = store.resumeStream('session-1', null, { force: true });
    const second = store.resumeStream('session-1', null, { force: true });
    assert.equal(store.isSessionBusy('session-1'), false);
    pending[1]();
    await second;
    pending[0]();
    await first;
    assert.equal(store.isSessionBusy('session-1'), false);
    assert.equal(ensureRuntime('session-1').resumeController, null);
    assert.equal(streamRequests, 0);
  } finally { store.loadSessionDetail = detail; chatWsClient.request = request; store.resetState(); }
});

for (const switching of [false, true]) {
  test(`a refresh never flashes a committed row back to running during ${switching ? 'thread switch' : 'refresh'}`, async () => {
    const store = await setup();
    const { default: api } = await import('../../src/api/http');
    const { threadViewSlots } = await import('../../src/realtime/chat/threadViewStore');
    const original = api.defaults.adapter;
    const pending: Array<() => void> = [];
    let opens = 0;
    api.defaults.adapter = async config => {
      const url = String(config.url || '');
      if (url.endsWith('/view/bootstrap')) {
        if (opens++ === 0) return viewResponse(config, {
          page: viewPage([viewRow()]), session: viewSession({ runtime_status: 'completed' })
        });
        return new Promise(resolve => pending.push(() => resolve(viewResponse(config, {
          page: viewPage([viewRow()]), session: viewSession({ runtime_status: 'completed' })
        }))));
      }
      return viewResponse(config, { events: [], rounds: [], running: false });
    };
    const slotStatus = () => (threadViewSlots('session-1') ?? [])[0]?.assistant.status;
    try {
      await store.loadSessionDetail('session-1', { startWatcherAfterHydration: false });
      assert.equal(store.isSessionBusy('session-1'), false);
      assert.equal(slotStatus(), 'completed');
      if (switching) store.activeSessionId = 'session-2';
      const loading = store.loadSessionDetail('session-1', { startWatcherAfterHydration: false });
      await new Promise(resolve => setTimeout(resolve, 0));
      // 刷新在飞行中时，已提交的行保持 settled，绝不闪回 running。
      assert.equal(store.isSessionBusy('session-1'), false);
      assert.equal(slotStatus(), 'completed');
      pending.splice(0).forEach(resolve => resolve());
      await loading;
      assert.equal(store.isSessionBusy('session-1'), false);
      assert.equal(slotStatus(), 'completed');
    } finally { pending.splice(0).forEach(resolve => resolve()); api.defaults.adapter = original; store.resetState(); }
  });
}

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
