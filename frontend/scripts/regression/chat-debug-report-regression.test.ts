import test from 'node:test';
import assert from 'node:assert/strict';
import { computed } from 'vue';
import { chatDebugLog, clearChatDebugHistory, getChatDebugStatus, isChatDebugEnabled, setChatDebugEnabled, setChatDebugVerboseEnabled } from '../../src/utils/chatDebug';
import { buildChatDebugReport } from '../../src/utils/chatDebugReport';

test('debug can be stopped even with a URL flag and blocked local storage', () => {
  const previousWindow = globalThis.window;
  const previousInfo = console.info;
  const location = new URL('https://example.invalid/?chat_debug=1');
  Object.assign(globalThis, { window: {
    location,
    localStorage: { getItem: () => { throw new Error('blocked'); }, setItem: () => { throw new Error('blocked'); }, removeItem: () => { throw new Error('blocked'); } },
    history: { state: null, replaceState: (_state: unknown, _title: string, url: string) => { location.href = String(url); } }
  } });
  console.info = () => {};
  try {
    setChatDebugEnabled(false);
    const observed = computed(() => isChatDebugEnabled());
    assert.equal(observed.value, false);
    setChatDebugVerboseEnabled(true);
    assert.equal(observed.value, true);
    chatDebugLog('messenger.viewport', 'fixture-event', { messageCount: 2 });
    assert.equal(getChatDebugStatus().historyCount, 1);
    location.search = '?chat_debug=1';
    setChatDebugEnabled(false);
    chatDebugLog('messenger.viewport', 'disabled-event');
    assert.equal(getChatDebugStatus().enabled, false);
    assert.equal(getChatDebugStatus().verbose, false);
    assert.equal(getChatDebugStatus().historyCount, 1);
    assert.equal(location.search, '');
  } finally {
    console.info = previousInfo;
    Object.assign(globalThis, { window: previousWindow });
  }
});

test('debug report stays bounded, filters raw content, and reflects new events on every export', () => {
  const previousWindow = globalThis.window;
  const previousInfo = console.info;
  const previousViewport = { innerWidth: globalThis.innerWidth, innerHeight: globalThis.innerHeight, devicePixelRatio: globalThis.devicePixelRatio };
  Object.assign(globalThis, { innerWidth: 1200, innerHeight: 800, devicePixelRatio: 1 });
  Object.assign(globalThis, { window: { location: { search: '' }, localStorage: {
    getItem: () => null, setItem: () => {}, removeItem: () => {}
  } } });
  console.info = () => {};
  try {
    setChatDebugVerboseEnabled(true);
    const payload: Record<string, unknown> = { status: 'running', messageCount: 3, contentLength: 120,
      content: 'fixture-private-value', session_id: 'fixture-private-value', nested: { args: 'fixture-private-value' }, list: ['fixture-private-value'] };
    payload.circular = payload;
    for (let index = 0; index < 2010; index++) chatDebugLog('chat.stream.perf', `event-${index}`, payload);
    const first = buildChatDebugReport();
    assert.equal(first.debug.events.length, 2000);
    assert.equal(first.debug.events[0].event, 'event-10');
    const text = JSON.stringify(first);
    assert.equal(text.includes('fixture-private-value'), false);
    assert.ok(text.length < 2_000_000);
    assert.equal((first.debug.events[0].payload as any).contentLength, 120);
    chatDebugLog('chat.stream.perf', 'new-event');
    assert.equal(buildChatDebugReport().debug.events.at(-1)?.event, 'new-event');
    assert.equal(clearChatDebugHistory(), 2000);
    assert.deepEqual(buildChatDebugReport().debug.events, []);
  } finally {
    setChatDebugEnabled(false);
    console.info = previousInfo;
    Object.assign(globalThis, previousViewport);
    Object.assign(globalThis, { window: previousWindow });
  }
});
