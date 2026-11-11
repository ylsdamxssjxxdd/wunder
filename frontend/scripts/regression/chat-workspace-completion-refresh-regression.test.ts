import test, { type TestContext } from 'node:test';
import assert from 'node:assert/strict';
import {
  refreshWorkspaceForThreadStatus,
  shouldRefreshWorkspaceForThreadStatus
} from '../../src/stores/chatWorkspaceEffects';

type RefreshDetail = {
  reason?: string;
  forceTree?: boolean;
  scope?: string;
  sessionId?: string;
};

const withRefreshEvents = (t: TestContext, run: (events: RefreshDetail[], tick: (ms: number) => void) => void) => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const events: RefreshDetail[] = [];
  const previousWindow = (globalThis as { window?: unknown }).window;
  const target = new EventTarget();
  (globalThis as { window?: unknown }).window = target;
  target.addEventListener('wunder:workspace-refresh', (event) => {
    events.push((event as CustomEvent<RefreshDetail>).detail || {});
  });
  t.after(() => {
    (globalThis as { window?: unknown }).window = previousWindow;
  });
  run(events, (ms) => t.mock.timers.tick(ms));
};

const store = { sessions: [{ id: 'sess-complete', agent_id: 'agent-1' }] };

test('a completed turn refreshes immediately and once more after the settle delay', (t) => {
  withRefreshEvents(t, (events, tick) => {
    refreshWorkspaceForThreadStatus(store, 'sess-complete', 'completed');
    assert.equal(events.length, 1);
    assert.equal(events[0].reason, 'thread_completed');
    assert.equal(events[0].forceTree, true);
    assert.equal(events[0].scope, 'directory');
    tick(1499);
    assert.equal(events.length, 1, 'no trailing refresh before the settle delay');
    tick(1);
    assert.equal(events.length, 2);
    assert.equal(events[1].reason, 'thread_completed_settle');
    assert.equal(events[1].forceTree, true);
  });
});

test('repeated terminal signals share one trailing refresh per session', (t) => {
  withRefreshEvents(t, (events, tick) => {
    refreshWorkspaceForThreadStatus(store, 'sess-complete', 'completed');
    tick(600);
    refreshWorkspaceForThreadStatus(store, 'sess-complete', 'completed');
    assert.equal(events.length, 2, 'both immediate boundaries are forwarded');
    tick(1499);
    assert.equal(events.length, 2);
    tick(1);
    assert.equal(events.length, 3, 'the trailing refresh is coalesced per session');
    assert.equal(events[2].reason, 'thread_completed_settle');
  });
});

test('non-terminal boundaries do not schedule a trailing refresh', (t) => {
  withRefreshEvents(t, (events, tick) => {
    refreshWorkspaceForThreadStatus(store, 'sess-complete', 'workspace_changed');
    assert.equal(events.length, 1);
    tick(2000);
    assert.equal(events.length, 1);
  });
});

test('only execution boundaries invalidate the directory snapshot', () => {
  assert.equal(shouldRefreshWorkspaceForThreadStatus('running', 'completed'), true);
  assert.equal(shouldRefreshWorkspaceForThreadStatus('running', 'waiting_approval'), true);
  assert.equal(shouldRefreshWorkspaceForThreadStatus('running', 'running'), false);
  assert.equal(shouldRefreshWorkspaceForThreadStatus(undefined, 'running'), false);
  assert.equal(shouldRefreshWorkspaceForThreadStatus('completed', 'idle'), false);
});
