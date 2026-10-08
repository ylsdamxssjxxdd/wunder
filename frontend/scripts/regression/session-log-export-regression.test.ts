import test from 'node:test';
import assert from 'node:assert/strict';

const storage = new Map<string, string>();
Object.assign(globalThis, {
  window: {
    location: {
      origin: 'http://localhost'
    }
  },
  localStorage: {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => {
      storage.set(key, String(value));
    },
    removeItem: (key: string) => {
      storage.delete(key);
    }
  }
});

const { normalizeSessionForLogExport } = require('../../src/utils/sessionLogExport');

test('session log export reads canonical transcript before legacy messages', () => {
  const session = normalizeSessionForLogExport('session-id', {
    id: 'session-id',
    title: 'title',
    transcript: [
      { role: 'user', content: 'question' },
      { role: 'assistant', content: 'answer' }
    ],
    messages: []
  });

  assert.equal(session.messageCount, 2);
  assert.equal(session.messages[0].role, 'user');
  assert.equal(session.messages[0].content, 'question');
});
