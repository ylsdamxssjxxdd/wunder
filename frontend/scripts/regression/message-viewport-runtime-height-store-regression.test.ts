import test from 'node:test';
import assert from 'node:assert/strict';

import { ref } from 'vue';

import {
  createMessageHeightEstimateStore,
  createMessageViewportRuntime,
  MESSAGE_VIRTUAL_HEIGHT_STORE_KEY,
  type HeightEstimateStorageLike
} from '../../src/views/messenger/messageViewportRuntime';

class FakeStorage implements HeightEstimateStorageLike {
  private readonly data = new Map<string, string>();

  getItem(key: string): string | null {
    return this.data.has(key) ? (this.data.get(key) as string) : null;
  }

  setItem(key: string, value: string): void {
    this.data.set(key, String(value));
  }

  removeItem(key: string): void {
    this.data.delete(key);
  }

  raw(key: string): string | null {
    return this.getItem(key);
  }
}

test('height estimate store keeps a bounded table per session', () => {
  const storage = new FakeStorage();
  const store = createMessageHeightEstimateStore({ storage, limitPerSession: 3, sessionLimit: 4 });

  store.save('session-a', new Map<string, number>([
    ['a', 100],
    ['b', 110],
    ['c', 120],
    ['d', 130]
  ]));

  const loaded = store.load('session-a');
  assert.equal(loaded.size, 3);
  assert.equal(loaded.has('a'), false);
  assert.equal(loaded.get('d'), 130);
  assert.deepEqual(store.sessions(), ['session-a']);

  // A single persist call must not push the stored footprint past the cap.
  const raw = JSON.parse(storage.raw(MESSAGE_VIRTUAL_HEIGHT_STORE_KEY) as string) as {
    sessions: Record<string, Record<string, number>>;
  };
  assert.equal(Object.keys(raw.sessions['session-a']).length, 3);
});

test('height estimate store evicts the least recently touched session', () => {
  const storage = new FakeStorage();
  const store = createMessageHeightEstimateStore({ storage, limitPerSession: 5, sessionLimit: 2 });

  store.save('session-a', new Map<string, number>([['a', 100]]));
  store.save('session-b', new Map<string, number>([['b', 100]]));
  store.load('session-a');
  store.save('session-c', new Map<string, number>([['c', 100]]));

  assert.deepEqual(store.sessions(), ['session-c', 'session-a']);
  assert.equal(store.load('session-b').size, 0);
  // The session count cap keeps the persisted payload bounded.
  const raw = JSON.parse(storage.raw(MESSAGE_VIRTUAL_HEIGHT_STORE_KEY) as string) as {
    order: string[];
  };
  assert.equal(raw.order.length, 2);
});

test('height estimate store ignores unreadable payloads', () => {
  const storage = new FakeStorage();
  storage.setItem(MESSAGE_VIRTUAL_HEIGHT_STORE_KEY, '{not json');
  const store = createMessageHeightEstimateStore({ storage, limitPerSession: 4, sessionLimit: 2 });

  assert.equal(store.load('session-a').size, 0);
  store.save('session-a', new Map<string, number>([['a', 100]]));
  assert.equal(store.load('session-a').get('a'), 100);
});

test('height estimate store drops a session on request', () => {
  const storage = new FakeStorage();
  const store = createMessageHeightEstimateStore({ storage, limitPerSession: 4, sessionLimit: 2 });
  store.save('session-a', new Map<string, number>([['a', 100]]));
  store.dropSession('session-a');
  assert.equal(store.load('session-a').size, 0);
  assert.deepEqual(store.sessions(), []);
});

test('viewport runtime hydrates and persists the height table per session', () => {
  const storage = new FakeStorage();
  const store = createMessageHeightEstimateStore({ storage, limitPerSession: 10, sessionLimit: 4 });
  store.save('agent:session-a', new Map<string, number>([['turn:1', 240]]));

  const messageVirtualHeightCache = new Map<string, number>();
  const runtime = createMessageViewportRuntime({
    messageListRef: ref(null),
    showChatSettingsView: ref(false),
    autoStickToBottom: ref(false),
    showScrollTopButton: ref(false),
    showScrollBottomButton: ref(false),
    isAgentConversationActive: ref(true),
    isWorldConversationActive: ref(false),
    activeConversationKey: ref('agent:session-a'),
    shouldVirtualizeMessages: ref(true),
    agentRenderableMessages: ref([]),
    worldRenderableMessages: ref([]),
    messageVirtualHeightCache,
    messageVirtualLayoutVersion: ref(0),
    messageVirtualScrollTop: ref(0),
    messageVirtualViewportHeight: ref(0),
    estimateVirtualOffsetTop: () => 0,
    resolveVirtualMessageHeight: () => 0,
    heightEstimateStore: store
  });

  // Creation hydrates the cache from the persisted table.
  assert.equal(messageVirtualHeightCache.get('turn:1'), 240);

  messageVirtualHeightCache.set('turn:2', 260);
  runtime.dispose();

  // Disposal writes the current table back under the active session.
  assert.equal(store.load('agent:session-a').get('turn:2'), 260);
  assert.equal(store.load('agent:session-a').get('turn:1'), 240);
});

test('viewport runtime persists the height table under an explicit session id', () => {
  const storage = new FakeStorage();
  const store = createMessageHeightEstimateStore({ storage, limitPerSession: 10, sessionLimit: 4 });
  const messageVirtualHeightCache = new Map<string, number>([['turn:9', 300]]);
  const runtime = createMessageViewportRuntime({
    messageListRef: ref(null),
    showChatSettingsView: ref(false),
    autoStickToBottom: ref(false),
    showScrollTopButton: ref(false),
    showScrollBottomButton: ref(false),
    isAgentConversationActive: ref(true),
    isWorldConversationActive: ref(false),
    activeConversationKey: ref('agent:session-b'),
    shouldVirtualizeMessages: ref(true),
    agentRenderableMessages: ref([]),
    worldRenderableMessages: ref([]),
    messageVirtualHeightCache,
    messageVirtualLayoutVersion: ref(0),
    messageVirtualScrollTop: ref(0),
    messageVirtualViewportHeight: ref(0),
    estimateVirtualOffsetTop: () => 0,
    resolveVirtualMessageHeight: () => 0,
    heightEstimateStore: store,
    heightEstimateSessionId: () => 'agent:session-c'
  });

  runtime.dispose();

  assert.equal(store.load('agent:session-c').get('turn:9'), 300);
  assert.equal(store.load('agent:session-b').size, 0);
});
