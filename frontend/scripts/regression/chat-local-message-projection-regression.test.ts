import test from 'node:test';
import assert from 'node:assert/strict';
import { createPinia, setActivePinia } from 'pinia';

const storage = new Map<string, string>();
(globalThis as Record<string, unknown>).localStorage = {
  getItem: (key: string) => storage.get(key) ?? null,
  setItem: (key: string, value: string) => {
    storage.set(key, String(value));
  },
  removeItem: (key: string) => {
    storage.delete(key);
  },
  clear: () => {
    storage.clear();
  }
};

test('appendLocalMessage materializes local command messages through runtime projection', async () => {
  const { useChatStore } = await import('../../src/stores/chat');
  setActivePinia(createPinia());
  const store = useChatStore();
  store.activeSessionId = 'session-local';
  store.sessions = [{ id: 'session-local', agent_id: 'agent-local' }];

  const localTurnId = 'local-command-turn:test';
  store.appendLocalMessage('user', '/help', { sessionId: 'session-local', localTurnId });
  store.appendLocalMessage('assistant', 'Available commands', {
    sessionId: 'session-local',
    localTurnId,
    localModelTurnId: `${localTurnId}:model`
  });

  const visible = store.visibleMessages('session-local') as Array<Record<string, unknown>>;
  assert.deepEqual(
    visible.map((message) => `${message.role}:${message.content}`),
    ['user:/help', 'assistant:Available commands']
  );
  assert.equal(visible.every((message) => message.__runtime_projected === true), true);
});
