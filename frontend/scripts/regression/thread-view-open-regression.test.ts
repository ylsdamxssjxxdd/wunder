import test from 'node:test';
import assert from 'node:assert/strict';
import { createPinia, setActivePinia } from 'pinia';

const values = new Map<string, string>();
Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: {
  getItem: (key: string) => values.get(key) ?? null,
  setItem: (key: string, value: string) => values.set(key, value),
  removeItem: (key: string) => values.delete(key)
} });
globalThis.requestAnimationFrame = () => 0;

const row = (revision = 1) => ({ root_turn_id: 'fixture-root', user_round: 1, revision,
  status: 'completed', user: { item_id: 'fixture-input', content: 'input' },
  assistant: { item_id: 'fixture-answer', content: `answer ${revision}` }, workflow_count: 0, failed_count: 0 });
const page = (key: string, seq = 1, empty = false) => ({ session_id: key, epoch: 1, view_seq: seq,
  projection_state: 'ready', thread_empty: empty, latest_round: empty ? 0 : 1,
  chat: empty ? [] : [row(seq)], trajectory: [], tiles: [], tile_level: 0, has_more: false, next_before: null });

async function fixture() {
  setActivePinia(createPinia());
  const { useChatStore } = await import('../../src/stores/chat');
  const { default: api } = await import('../../src/api/http');
  const loader = await import('../../src/stores/threadViewLoader');
  const views = await import('../../src/realtime/chat/threadViewStore');
  const store = useChatStore();
  store.resetState();
  const original = api.defaults.adapter;
  const pending: Array<{ key: string; signal: any; reply: (data?: any) => void }> = [];
  api.defaults.adapter = config => new Promise(resolve => {
    const key = String(config.url).split('/')[3];
    pending.push({ key, signal: config.signal, reply: (view = page(key)) => resolve({
      config, status: 200, statusText: 'OK', headers: {}, data: { data: { session: { id: key, agent_id: '' }, page: view } }
    }) });
  });
  const tick = () => new Promise(resolve => setTimeout(resolve, 0));
  return { store, loader, views, pending, tick, cleanup() { api.defaults.adapter = original; store.resetState(); } };
}
const options = { startWatcherAfterHydration: false };

test('concurrent opens share one bounded bootstrap and preserve a newer recovery page', async () => {
  const f = await fixture();
  try {
    const first = f.loader.openThreadView(f.store, 'fixture-a', options);
    const second = f.loader.openThreadView(f.store, 'fixture-a', options);
    await f.tick();
    assert.equal(f.pending.length, 1);
    f.views.commitThreadView('fixture-a', page('fixture-a', 2), f.views.threadViewGeneration('fixture-a'));
    f.pending[0].reply();
    assert.equal((await first).id, 'fixture-a');
    assert.equal((await second).id, 'fixture-a');
    assert.equal(f.store.messages[1].content, 'answer 2');
    assert.equal(f.views.threadViewPhase('fixture-a'), 'ready');
  } finally { f.cleanup(); }
});

test('A to B to A starts a fresh request before cancelled promises settle', async () => {
  const f = await fixture();
  try {
    const first = f.loader.openThreadView(f.store, 'fixture-a', options); await f.tick();
    const middle = f.loader.openThreadView(f.store, 'fixture-b', options); await f.tick();
    const last = f.loader.openThreadView(f.store, 'fixture-a', options); await f.tick();
    assert.equal(f.pending.length, 3);
    assert.equal(f.pending[0].signal.aborted, true);
    assert.equal(f.pending[1].signal.aborted, true);
    f.pending[2].reply(page('fixture-a', 3));
    assert.equal((await last).id, 'fixture-a');
    f.pending[0].reply(); f.pending[1].reply();
    assert.equal(await first, null); assert.equal(await middle, null);
    assert.equal(f.store.activeSessionId, 'fixture-a');
    assert.equal(f.store.messages[1].content, 'answer 3');
  } finally { f.cleanup(); }
});

test('a foreground caller joining a recovery open still starts the watcher', async () => {
  const f = await fixture();
  const { chatWsClient } = await import('../../src/stores/chatWatcher');
  const original = chatWsClient.request;
  let watches = 0;
  chatWsClient.request = (request: any) => {
    watches++;
    return new Promise((_, reject) => request.signal.addEventListener('abort', () => {
      const error = new Error('cancelled'); error.name = 'AbortError'; reject(error);
    }, { once: true }));
  };
  try {
    const first = f.loader.openThreadView(f.store, 'fixture-a', options);
    const foreground = f.loader.openThreadView(f.store, 'fixture-a');
    await f.tick(); f.pending[0].reply();
    await Promise.all([first, foreground]);
    assert.equal(watches, 1);
    assert.equal(f.pending.length, 1);
  } finally { f.cleanup(); chatWsClient.request = original; }
});

test('reset cancels pending opens and does not reuse generations for the same thread', async () => {
  const f = await fixture();
  try {
    const old = f.loader.openThreadView(f.store, 'fixture-a', options); await f.tick();
    const generation = f.views.threadViewGeneration('fixture-a');
    f.store.resetState();
    const current = f.loader.openThreadView(f.store, 'fixture-a', options); await f.tick();
    assert.ok(f.views.threadViewGeneration('fixture-a') > generation);
    f.pending[1].reply(page('fixture-a', 0, true));
    assert.equal((await current).id, 'fixture-a');
    f.pending[0].reply(); assert.equal(await old, null);
    assert.equal(f.views.threadViewPhase('fixture-a'), 'verified_empty');
    assert.equal(f.store.messages.length, 0);
  } finally { f.cleanup(); }
});

test('invalid bootstrap still fails even with an existing transcript', async () => {
  const f = await fixture();
  try {
    f.views.commitThreadView('fixture-a', page('fixture-a', 2), f.views.beginThreadView('fixture-a'));
    const pending = f.loader.openThreadView(f.store, 'fixture-a', options);
    const rejected = assert.rejects(pending, /响应无效/);
    await f.tick(); f.pending[0].reply({ ...page('fixture-a'), chat: null }); await rejected;
    assert.equal(f.views.threadViewSlots('fixture-a')![0].assistant.content, 'answer 2');
  } finally { f.cleanup(); }
});
