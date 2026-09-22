import test from 'node:test';
import assert from 'node:assert/strict';
import { createPinia, setActivePinia } from 'pinia';

const values = new Map<string, string>();
Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: {
  getItem: (key: string) => values.get(key) ?? null,
  setItem: (key: string, value: string) => values.set(key, value),
  removeItem: (key: string) => values.delete(key), clear: () => values.clear()
} });
const session = (id: string, agent_id = '') => ({ id, agent_id, status: 'active',
  created_at: '2026-01-01T00:00:00.000Z', last_message_at: '2026-01-01T00:00:00.000Z' });
const setup = async () => {
  setActivePinia(createPinia());
  const { useChatStore } = await import('../../src/stores/chat');
  const store = useChatStore();
  store.resetState();
  return store;
};

test('server deletion clears the active catalog and all caches; stale pages cannot resurrect it', async () => {
  const store = await setup();
  const { default: api } = await import('../../src/api/http');
  const { writeSessionListCache, readSessionListCache } = await import('../../src/stores/chatRuntimeState');
  const { mergeSessionCatalogPage } = await import('../../src/stores/chatSessionCatalog');
  store.sessions = [session('session-a'), session('session-b')];
  store.activeSessionId = 'session-a';
  store.messages = [{ role: 'user', content: 'input' }];
  writeSessionListCache('', store.sessions);
  writeSessionListCache('__all_sessions__', store.sessions);
  const original = api.defaults.adapter;
  api.defaults.adapter = async config => {
    assert.ok(config.params.known_session_ids.includes('session-a'));
    return { status: 200, statusText: 'OK', headers: {}, config,
      data: { data: { total: 200, items: [session('session-b')], unavailable_session_ids: ['session-a'] } } };
  };
  try {
    await store.loadSessions({ force: true });
    mergeSessionCatalogPage(store, { items: [session('session-a')] });
    assert.deepEqual([store.activeSessionId, store.sessions.map(item => item.id)], [null, ['session-b']]);
    for (const key of ['', '__all_sessions__']) {
      assert.equal(readSessionListCache(key).some(item => item.id === 'session-a'), false);
    }
  } finally { api.defaults.adapter = original; store.resetState(); }
});

test('pagination and concurrent creation preserve entries unless explicitly checked and rejected', async () => {
  const store = await setup();
  const { mergeSessionCatalogPage } = await import('../../src/stores/chatSessionCatalog');
  store.sessions = [session('session-a'), session('session-b', 'agent-b'), session('session-new')];
  mergeSessionCatalogPage(store, { items: [], unavailable_session_ids: ['session-new'] }, ['session-a']);
  assert.deepEqual(store.sessions.map(item => item.id).sort(), ['session-a', 'session-b', 'session-new']);
  store.resetState();
});

test('large catalogs reconcile in bounded rotating batches including the active thread', async () => {
  const store = await setup();
  const { sessionCatalogCheckIds } = await import('../../src/stores/chatSessionCatalog');
  store.sessions = Array.from({ length: 250 }, (_, i) => session(`session-${i}`));
  store.activeSessionId = 'session-249';
  const checked = new Set<string>();
  for (let i = 0; i < 3; i++) {
    const ids = sessionCatalogCheckIds(store, null);
    assert.ok(ids.length <= 100);
    assert.ok(ids.includes(store.activeSessionId));
    ids.forEach(id => checked.add(id));
  }
  assert.equal(checked.size, 250);
  store.resetState();
});

test('new thread reuses the same agent draft after switching and excludes consumed or running threads', async () => {
  const store = await setup();
  const { isReusableFreshSession } = await import('../../src/stores/chatRuntimeState');
  store.sessions = [session('session-draft', 'agent-a'), { ...session('session-used', 'agent-a'),
    last_message_at: '2026-01-01T00:00:00.001Z' }];
  store.activeSessionId = 'session-used';
  assert.equal(store.resolveReusableFreshSessionId('agent-a'), 'session-draft');
  assert.equal(store.resolveReusableFreshSessionId('agent-b'), '');
  store.loadingBySession['session-draft'] = true;
  assert.equal(store.resolveReusableFreshSessionId('agent-a'), '');
  for (const patch of [{ status: 'archived' }, { parent_session_id: 'parent-a' },
    { consumed_tokens: 1 }, { tool_calls: 1 }, { created_at: null }]) {
    assert.equal(isReusableFreshSession({ ...session('session-c'), ...patch }), false);
  }
  assert.equal(isReusableFreshSession(session('session-c'), [{ role: 'user', attachments: [{}] }]), false);
  store.resetState();
});

test('explicit restoration makes an unavailable thread visible again', async () => {
  const store = await setup();
  const { mergeSessionCatalogPage, restoreSessionCatalogEntry } = await import('../../src/stores/chatSessionCatalog');
  store.sessions = [session('session-a')];
  mergeSessionCatalogPage(store, { unavailable_session_ids: ['session-a'] }, ['session-a']);
  restoreSessionCatalogEntry(store, 'session-a');
  mergeSessionCatalogPage(store, { items: [session('session-a')] });
  assert.deepEqual(store.sessions.map(item => item.id), ['session-a']);
  store.resetState();
});
