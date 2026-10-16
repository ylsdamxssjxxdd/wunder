import test from 'node:test';
import assert from 'node:assert/strict';
import { createPinia, setActivePinia } from 'pinia';

const storage = new Map<string, string>();
Object.defineProperty(globalThis, 'localStorage', {
  configurable: true,
  value: {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, String(value)),
    removeItem: (key: string) => storage.delete(key),
    clear: () => storage.clear()
  }
});

const createRequest = <T>(result: T) => {
  const request: { result?: T; onsuccess?: () => void; onerror?: () => void } = {};
  queueMicrotask(() => {
    request.result = result;
    request.onsuccess?.();
  });
  return request;
};

const database = {
  objectStoreNames: { contains: () => true },
  transaction: () => ({ objectStore: () => ({ getAll: () => createRequest([]) }) })
};
Object.defineProperty(globalThis, 'indexedDB', {
  configurable: true,
  value: {
    open: () => {
      const request: {
        result?: typeof database;
        onsuccess?: () => void;
        onupgradeneeded?: () => void;
      } = {};
      queueMicrotask(() => {
        request.result = database;
        request.onsuccess?.();
      });
      return request;
    }
  }
});

const globalCompanion = {
  id: 'global-companion',
  display_name: 'Global companion',
  spritesheet_path: 'spritesheet.webp',
  spritesheet_url: '/companions/global-companion/spritesheet'
};

test('selected global companion is restored after a page reload', async () => {
  const [{ default: api }, { useCompanionStore }] = await Promise.all([
    import('../../src/api/http'),
    import('../../src/stores/companions')
  ]);
  const http = api as unknown as { get: (url: string) => Promise<unknown> };
  http.get = async () => ({ data: { data: { items: [globalCompanion] } } });

  setActivePinia(createPinia());
  const firstSessionStore = useCompanionStore();
  await firstSessionStore.loadGlobalCompanions();
  firstSessionStore.selectCompanion(globalCompanion.id);
  assert.equal(firstSessionStore.settings.selectedId, globalCompanion.id);

  // A fresh Pinia instance models the store being recreated by a full page reload.
  setActivePinia(createPinia());
  const reloadedStore = useCompanionStore();
  await reloadedStore.hydrate();

  assert.equal(reloadedStore.settings.selectedId, globalCompanion.id);
  assert.equal(reloadedStore.selectedCompanion?.id, globalCompanion.id);
  assert.equal(reloadedStore.enabled, true);
});
