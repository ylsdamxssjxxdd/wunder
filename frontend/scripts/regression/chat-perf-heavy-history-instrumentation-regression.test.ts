// W4-L heavy-history instrumentation gate.
//
// The sibling chat-perf-heavy-history-regression.test.ts freezes the six metric
// names and the chatPerf recording surface. This file covers the other half of
// W4-L: the six hot paths must actually call those recorders. Each test drives
// one instrumented path with a capture running and asserts exactly one record
// lands, so deleting an instrumentation call turns the matching test red.
import test from 'node:test';
import assert from 'node:assert/strict';

import { ref } from 'vue';

import { chatPerf } from '../../src/utils/chatPerf';
import {
  applyChatThreadSnapshot,
  applyChatThreadSnapshotPlanSharded
} from '../../src/realtime/chat/chatThreadState';
import type {
  ChatThreadSnapshot,
  ChatThreadSnapshotPlan
} from '../../src/realtime/chat/chatThreadState';
import { emptyChatThreadState } from '../../src/realtime/chat/chatThreadTypes';
import { buildChatThreadTurnSlots } from '../../src/realtime/chat/chatThreadProjection';
import {
  applyChatThreadServerEvent,
  registerChatThreadSnapshotLoader,
  resetChatThreadRuntime
} from '../../src/realtime/chat/chatThreadRuntime';
import { deriveTimeline } from '../../src/views/trajectory/trajectoryModel';
import { createMessageViewportRuntime } from '../../src/views/messenger/messageViewportRuntime';

// The store/api graph touches `localStorage` and `navigator` at module scope, so
// the workflow-events test installs minimal stubs and then imports it lazily.
// Keeping the import dynamic also keeps the five DOM-free tests above free of
// that dependency chain.
const installNodeBrowserStubs = () => {
  const backing = new Map<string, string>();
  const globals = globalThis as unknown as Record<string, unknown>;
  if (!globals.localStorage) {
    globals.localStorage = {
      getItem: (key: string) => (backing.has(key) ? backing.get(key) : null),
      setItem: (key: string, value: string) => { backing.set(key, String(value)); },
      removeItem: (key: string) => { backing.delete(key); },
      clear: () => { backing.clear(); },
      key: () => null,
      length: 0
    };
  }
  if (!globals.navigator) {
    globals.navigator = { language: 'zh-CN', languages: ['zh-CN'], userAgent: 'node' };
  }
  if (!globals.document) {
    globals.document = { documentElement: { lang: '' } };
  }
};

const durationCount = (name: string): number => chatPerf.snapshot().durations[name]?.count ?? 0;
const counterValue = (name: string): number => chatPerf.snapshot().counters[name] ?? 0;

const delay = (ms = 0) => new Promise<void>((resolve) => { setTimeout(resolve, ms); });

const turnSnapshot = (cursor: number, sessionId: string): ChatThreadSnapshot => ({
  cursor,
  turns: [{ turn_id: `${sessionId}:t1`, user_round: 1, status: 'completed', content: 'hello' }],
  items: [],
  blocks: []
});

test('applyChatThreadSnapshot records exactly one snapshot_apply_ms per apply', () => {
  const state = emptyChatThreadState('perf-snapshot-sync');
  chatPerf.start();
  const before = durationCount('snapshot_apply_ms');
  applyChatThreadSnapshot(state, turnSnapshot(4, 'perf-snapshot-sync'), 0);
  const after = durationCount('snapshot_apply_ms');
  chatPerf.stop();

  assert.equal(state.lastSeq, 4);
  assert.equal(after - before, 1, 'applyChatThreadSnapshot must record one snapshot_apply_ms');
});

test('applyChatThreadSnapshotPlanSharded records once even when it yields mid-apply', async () => {
  const state = emptyChatThreadState('perf-snapshot-sharded');
  const plan: ChatThreadSnapshotPlan = {
    cursor: 6,
    turnCount: 1,
    itemCount: 0,
    blockCount: 0,
    shards: [
      { kind: 'turns', cost: 1, turns: [{ turn_id: 't1', user_round: 1, status: 'running', content: 'q' }] },
      { kind: 'items', cost: 1, items: [] },
      { kind: 'blocks', cost: 1, blocks: [] }
    ]
  };

  chatPerf.start();
  const before = durationCount('snapshot_apply_ms');
  const outcome = await applyChatThreadSnapshotPlanSharded(state, plan, {
    yieldBudget: 1,
    yieldToMain: () => delay(0)
  });
  const after = durationCount('snapshot_apply_ms');
  chatPerf.stop();

  assert.equal(outcome.applied, true);
  assert.ok(outcome.yields >= 1, 'the sharded apply must have yielded at least once');
  assert.equal(after - before, 1, 'the sharded apply must record exactly one snapshot_apply_ms');
});

test('buildChatThreadTurnSlots records exactly one thread_projection_ms', () => {
  const state = emptyChatThreadState('perf-projection');
  applyChatThreadSnapshot(state, turnSnapshot(3, 'perf-projection'), 0);

  chatPerf.start();
  const before = durationCount('thread_projection_ms');
  const slots = buildChatThreadTurnSlots(state);
  const after = durationCount('thread_projection_ms');
  chatPerf.stop();

  assert.equal(slots.length, 1);
  assert.equal(after - before, 1, 'buildChatThreadTurnSlots must record one thread_projection_ms');
});

test('deriveTimeline records exactly one trajectory_layout_ms', () => {
  chatPerf.start();
  const before = durationCount('trajectory_layout_ms');
  const model = deriveTimeline([]);
  const after = durationCount('trajectory_layout_ms');
  chatPerf.stop();

  assert.equal(model, null);
  assert.equal(after - before, 1, 'deriveTimeline must record one trajectory_layout_ms');
});

test('scheduleMessageViewportRefresh records one virtual_window_recompute_ms on the synchronous path', () => {
  const runtime = createMessageViewportRuntime({
    messageListRef: ref(null),
    showChatSettingsView: ref(false),
    autoStickToBottom: ref(true),
    showScrollTopButton: ref(false),
    showScrollBottomButton: ref(false),
    isAgentConversationActive: ref(true),
    isWorldConversationActive: ref(false),
    activeConversationKey: ref('perf-viewport'),
    shouldVirtualizeMessages: ref(true),
    agentRenderableMessages: ref([]),
    worldRenderableMessages: ref([]),
    messageVirtualHeightCache: new Map<string, number>(),
    messageVirtualLayoutVersion: ref(0),
    messageVirtualScrollTop: ref(0),
    messageVirtualViewportHeight: ref(0),
    estimateVirtualOffsetTop: () => 0,
    resolveVirtualMessageHeight: () => 0
  });

  chatPerf.start();
  const before = durationCount('virtual_window_recompute_ms');
  // No `window` in this host, so the refresh takes its synchronous recompute path.
  runtime.scheduleMessageViewportRefresh({});
  const after = durationCount('virtual_window_recompute_ms');
  chatPerf.stop();
  runtime.dispose();

  assert.equal(after - before, 1, 'scheduleMessageViewportRefresh must record one virtual_window_recompute_ms');
});

test('a failed snapshot_required hydrate counts one snapshot_abort', async () => {
  const key = 'perf-snapshot-abort';
  registerChatThreadSnapshotLoader(() => Promise.reject(new Error('perf loader failure')));

  chatPerf.start();
  const before = counterValue('snapshot_abort');
  applyChatThreadServerEvent({}, key, 'thread_snapshot_required', { cursor: 1 }, {});
  await delay(10);
  const after = counterValue('snapshot_abort');
  chatPerf.stop();

  registerChatThreadSnapshotLoader(null);
  resetChatThreadRuntime(key);

  assert.equal(after - before, 1, 'a failed hydrate must count one snapshot_abort');
});

test('loadSessionWorkflowEventsSnapshot counts the workflow rows it loaded', async () => {
  installNodeBrowserStubs();
  const httpModule = await import('../../src/api/http') as unknown as { default?: any };
  const storeModule = await import('../../src/stores/chatRuntimeState') as unknown as {
    loadSessionWorkflowEventsSnapshot: (sessionId: string, options?: Record<string, unknown>) => Promise<any>;
  };
  const api = httpModule.default;
  assert.ok(api, 'the axios instance must be importable with the stubs in place');

  const originalAdapter = api.defaults.adapter;
  const rows = [
    { event_type: 'tool_call_started' },
    { event_type: 'tool_call_finished' },
    { event_type: 'workflow_event' }
  ];
  api.defaults.adapter = async (config: any) => ({
    data: { data: { workflow_only: true, events: rows } },
    status: 200,
    statusText: 'OK',
    headers: {},
    config
  });

  try {
    chatPerf.start();
    const before = counterValue('workflow_events_items');
    const payload = await storeModule.loadSessionWorkflowEventsSnapshot('perf-workflow-1', {});
    const after = counterValue('workflow_events_items');
    chatPerf.stop();

    assert.ok(payload, 'the loader must return the workflow snapshot payload');
    assert.equal(after - before, rows.length, 'every loaded workflow row must be counted');
  } finally {
    api.defaults.adapter = originalAdapter;
  }
});
