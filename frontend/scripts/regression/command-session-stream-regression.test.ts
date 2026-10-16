import test from 'node:test';
import assert from 'node:assert/strict';
import { createPinia, setActivePinia } from 'pinia';
import { useCommandSessionStore } from '../../src/stores/commandSessions';
import { commandSessionOutputPreview, resolveTimelineCommandSessionRef } from '../../src/components/chat/toolTimelineCommandSession';
import type { RawToolRun } from '../../src/components/chat/toolWorkflowRunModel';

test('live output survives duplicate send/watch delivery and snapshot replay', () => {
  setActivePinia(createPinia());
  const store = useCommandSessionStore();
  store.upsertSnapshot('thread', { command_session_id: 'command', seq: 0 });
  store.appendDelta('thread', 'command', 'stdout', 'first\n', { seq: 1, tool_call_id: 'call' });
  assert.equal(store.appendDelta('thread', 'command', 'stdout', 'first\n', { seq: 1 }), null);
  assert.equal(store.getById('command')?.stdoutTail, 'first\n');
  store.upsertSnapshot('thread', {
    command_session_id: 'command', seq: 2, stdout_tail: 'first\nsecond\n', status: 'running'
  });
  assert.equal(store.appendDelta('thread', 'command', 'stdout', 'second\n', { seq: 2 }), null);
  store.appendDelta('thread', 'command', 'stderr', 'warning\n', { seq: 3 });
  store.upsertSnapshot('thread', { command_session_id: 'command', seq: 4, status: 'exited', exit_code: 0 });
  assert.equal(store.getById('command')?.stdoutTail, 'first\nsecond\n');
  assert.equal(store.getByToolCallId('call')?.stderrTail, 'warning\n');
  assert.equal(store.getById('command')?.status, 'exited');
});

test('hydration and rendered output stay bounded while retaining the latest line', () => {
  setActivePinia(createPinia());
  const store = useCommandSessionStore();
  store.upsertSnapshot('thread', {
    command_session_id: 'command', seq: 1,
    stdout_tail: `first\n${'x'.repeat(80000)}\nlast\n`
  });
  const tail = store.getById('command')?.stdoutTail ?? '';
  assert.ok(tail.length < 25000);
  const preview = commandSessionOutputPreview(tail);
  assert.ok(preview.length < 4100);
  assert.ok(preview.startsWith('first\n'));
  assert.ok(preview.endsWith('\nlast\n'));
});

test('a delayed snapshot cannot replace newer output while keeping its cursor', () => {
  setActivePinia(createPinia());
  const store = useCommandSessionStore();
  store.upsertSnapshot('thread', {
    command_session_id: 'command', seq: 1, stdout_tail: 'first\n'
  });
  store.appendDelta('thread', 'command', 'stdout', 'second\n', { seq: 2 });
  store.upsertSnapshot('thread', {
    command_session_id: 'command', seq: 1, stdout_tail: 'first\n'
  });
  assert.equal(store.getById('command')?.seq, 2);
  assert.equal(store.getById('command')?.stdoutTail, 'first\nsecond\n');
  assert.equal(store.appendDelta('thread', 'command', 'stdout', 'second\n', { seq: 2 }), null);
});

test('timeline resolves a command poll and an execute result to the same live session', () => {
  const run: RawToolRun = {
    key: 'call', toolName: 'command_session', toolRuntimeName: 'command_session',
    toolDisplayName: '', toolFunctionName: '', outputItem: null, resultItem: null,
    callItem: { detail: JSON.stringify({ args: { command_session_id: 'command' } }) }
  };
  assert.equal(resolveTimelineCommandSessionRef(run), 'command');
  assert.equal(resolveTimelineCommandSessionRef({ ...run, callItem: null,
    resultItem: { detail: JSON.stringify({ ok: true, data: { command_session_id: 'command', status: 'running' } }) }
  }), 'command');
});
