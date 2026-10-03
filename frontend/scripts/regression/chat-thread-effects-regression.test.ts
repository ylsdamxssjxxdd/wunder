import test from 'node:test';
import assert from 'node:assert/strict';
import { createPinia, setActivePinia } from 'pinia';
import { applyChatThreadServerEvent, submitChatThreadTurn, hydrateChatThreadRuntime, resetChatThreadRuntime, buildChatThreadMaterializedMessages } from '../../src/realtime/chat/chatThreadRuntime';
import { buildWorkflowToolRuns } from '../../src/components/chat/toolWorkflowRunModel';
import { resolveCollapsedWorkflowEntryMetadata } from '../../src/components/chat/toolWorkflowCollapsedMetadata';

const key = 'thread-effects-test';
const setup = async () => {
  Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: {
    getItem: () => null, setItem: () => {}, removeItem: () => {}
  } });
  const { useChatStore } = await import('../../src/stores/chat');
  const { applyChatThreadEffects, syncChatThreadShell } = await import('../../src/stores/chatThreadEffects');
  setActivePinia(createPinia());
  resetChatThreadRuntime(key);
  const chat = useChatStore();
  chat.activeSessionId = key;
  chat.sessions = [{ id: key, agent_id: 'agent-example' }] as any;
  const events: string[] = [];
  const eventDetails: any[] = [];
  const original = globalThis.window;
  globalThis.window = { dispatchEvent: (event: Event) => { events.push(event.type); eventDetails.push((event as CustomEvent).detail); return true; } } as any;
  const apply = (seq: number, change_type: string, data: Record<string, unknown>) =>
    applyChatThreadServerEvent(chat, key, 'thread_change', {
      cursor: seq, change_type, turn_id: data.turn_id, item_id: data.item_id, revision: data.revision, payload: data
    }, { onChangesApplied: changes => applyChatThreadEffects(chat, key, changes) });
  return { chat, events, eventDetails, apply, syncChatThreadShell, cleanup: () => { globalThis.window = original; } };
};

test('durable lifecycle settles shell and dispatches workspace effects once even through replay and gaps', async () => {
  const { chat, events, apply, syncChatThreadShell, cleanup } = await setup();
  try {
    submitChatThreadTurn(chat, key, 'client-one', 'question');
    chat.loadingBySession[key] = true;
    syncChatThreadShell(chat, key);
    assert.equal(chat.isSessionBusy(key), true);
    apply(1, 'turn_upsert', { turn_id: 'turn-one', user_round: 1, status: 'running', client_message_id: 'client-one' });
    const workspace = { turn_id: 'turn-one', item_id: 'workspace-one', kind: 'workspace_update', event_type: 'workspace_update', revision: 1, status: 'completed', paths: ['example.txt'] };
    apply(3, 'item_upsert', workspace);
    assert.equal(events.includes('wunder:workspace-refresh'), false);
    apply(2, 'item_upsert', { turn_id: 'turn-one', item_id: 'turn-one:user', kind: 'user_message', content: 'question', revision: 1 });
    apply(3, 'item_upsert', workspace);
    assert.equal(events.filter(event => event === 'wunder:workspace-refresh').length, 1);
    apply(4, 'turn_upsert', { turn_id: 'turn-one', status: 'completed' });
    // The feeder may represent the same terminal transition as a later
    // turn_status frame. It must not emit a second completion side effect.
    apply(5, 'turn_status', { turn_id: 'turn-one', status: 'completed' });
    apply(6, 'turn_status', { turn_id: 'turn-one', status: 'completed' });
    assert.equal(chat.isSessionBusy(key), false);
    assert.equal(chat.sessionRuntimeStatus(key), 'completed');
    assert.equal(chat.isSessionLoading(key), false);
    assert.equal(events.filter(event => event === 'wunder:agent-runtime-refresh').length, 1);
    submitChatThreadTurn(chat, key, 'client-two', 'next question');
    const rows = buildChatThreadMaterializedMessages(key)!;
    assert.deepEqual(rows.map(row => row.role), ['user', 'assistant', 'user', 'assistant']);
    assert.equal(rows[1].final, true);
    assert.equal(rows[3].final, false);
  } finally { cleanup(); }
});

test('row-shaped atomic snapshot restores compact input and complete tool invocation metadata', async () => {
  const { chat, syncChatThreadShell, cleanup } = await setup();
  try {
    hydrateChatThreadRuntime(chat, key, {
      cursor: 8,
      turns: [{ turn_id: 'turn-compact', user_turn_index: 2, status: 'completed', payload: { user_round: 2 } }],
      items: [
        { item_id: 'turn-compact:user', turn_id: 'turn-compact', kind: 'user_message', revision: 2, status: 'completed', payload: { role: 'user', content: '/compact', user_round: 2 } },
        { item_id: 'turn-compact:tool-example', turn_id: 'turn-compact', kind: 'tool_call', revision: 3, status: 'completed', payload: {
          event_type: 'tool_result', tool: 'example_tool', tool_call_id: 'call-example', model_round: 1,
          args: { text: 'sample input' }, data: { result: 'sample result' }, meta: { duration_ms: 125 },
          request_context_tokens: 42, request_usage: { total_tokens: 57 }
        } }
      ], blocks: []
    });
    syncChatThreadShell(chat, key);
    const rows = buildChatThreadMaterializedMessages(key)!;
    assert.deepEqual(rows.map(row => row.role), ['user', 'assistant']);
    assert.equal(rows[0].content, '/compact');
    assert.equal(rows[1].final, true);
    const runs = buildWorkflowToolRuns(rows[1].workflowItems!);
    assert.equal(runs.length, 1);
    assert.deepEqual(JSON.parse(String(runs[0].callItem?.toolCallRawDetail)), { tool: 'example_tool', arguments: { text: 'sample input' } });
    const metadata = resolveCollapsedWorkflowEntryMetadata(runs[0]);
    assert.equal(metadata.durationLabel, '125ms');
    assert.equal(metadata.contextTokensLabel, '42 token');
    assert.equal(metadata.consumedTokensLabel, '57 token');
  } finally { cleanup(); }
});


test('production send callback binds its pending pair and settles from server turn_upsert before transport cleanup', async () => {
  const { chat, cleanup } = await setup();
  const { chatWsClient } = await import('../../src/stores/chatWatcher');
  const { clearSessionWatcher } = await import('../../src/stores/chatRuntimeControls');
  const originalRequest = chatWsClient.request;
  let pendingRoles: string[] = [];
  chatWsClient.request = (async (options: any) => {
    const request = typeof options.message === 'function' ? options.message() : options.message;
    if (request.type !== 'start') {
      return new Promise(resolve => options.signal?.addEventListener('abort', () => resolve(undefined), { once: true }));
    }
    pendingRoles = buildChatThreadMaterializedMessages(key)!.map(row => row.role);
    const clientId = request.payload.client_message_id;
    const emit = (cursor: number, payload: Record<string, unknown>, change_type = 'turn_upsert') => options.onEvent('thread_change', JSON.stringify({
      cursor, change_type, turn_id: 'turn-send', item_id: payload.item_id, revision: payload.revision, payload
    }));
    emit(1, { turn_id: 'turn-send', status: 'queued', user_round: 1, client_message_id: clientId });
    emit(2, { item_id: 'turn-send:user', turn_id: 'turn-send', kind: 'user_message', content: 'question', user_round: 1, client_message_id: clientId, revision: 1 }, 'item_upsert');
    emit(3, { turn_id: 'turn-send', status: 'running' });
    emit(4, { item_id: 'turn-send:text-1', turn_id: 'turn-send', model_round: 1, kind: 'assistant_message', role: 'assistant', content: 'answer', status: 'completed', revision: 1 }, 'item_upsert');
    emit(5, { turn_id: 'turn-send', status: 'completed' });
    assert.equal(chat.isSessionBusy(key), false);
    assert.equal(chat.isSessionLoading(key), false);
    options.onEvent('final', JSON.stringify({ content: 'answer' }));
  }) as any;
  try {
    await chat.sendMessage('question');
    assert.deepEqual(pendingRoles, ['user', 'assistant']);
    const rows = buildChatThreadMaterializedMessages(key)!;
    assert.deepEqual(rows.map(row => row.role), ['user', 'assistant']);
    assert.equal(rows[1].content, 'answer');
    assert.equal(chat.sessionRuntimeStatus(key), 'completed');
  } finally {
    clearSessionWatcher();
    chatWsClient.request = originalRequest;
    cleanup();
  }
});


test('scheduled rejection preserves Stop for active work and terminal bubbles retain their own output', async () => {
  const { chat, apply, cleanup } = await setup();
  try {
    apply(1, 'turn_upsert', { turn_id: 'active-turn', user_round: 1, status: 'running', content: 'Fixture task' });
    apply(2, 'item_upsert', { turn_id: 'active-turn', item_id: 'active-turn:text-1', kind: 'assistant_message',
      role: 'assistant', model_round: 1, revision: 1, status: 'running', content: 'Retained partial reply' });
    apply(3, 'turn_upsert', { turn_id: 'scheduled-turn', user_round: 2, status: 'queued', content: 'Fixture schedule' });
    apply(4, 'turn_upsert', { turn_id: 'scheduled-turn', status: 'rejected' });
    apply(5, 'item_upsert', { turn_id: 'scheduled-turn', item_id: 'scheduled-turn:terminal', kind: 'terminal',
      revision: 1, status: 'rejected', error: { code: 'USER_BUSY', message: 'Fixture busy' } });
    assert.equal(chat.isSessionBusy(key), true);
    let rows = buildChatThreadMaterializedMessages(key)!;
    assert.equal(rows[1].content, 'Retained partial reply');
    assert.equal(rows[1].status, 'streaming');
    assert.equal(rows[3].status, 'failed');
    apply(6, 'turn_upsert', { turn_id: 'active-turn', status: 'cancelled' });
    assert.equal(chat.isSessionBusy(key), false);
    rows = buildChatThreadMaterializedMessages(key)!;
    assert.equal(rows[1].content, 'Retained partial reply');
    assert.equal(rows[1].status, 'cancelled');
    assert.equal(rows[3].status, 'failed');
    apply(7, 'turn_upsert', { turn_id: 'result-turn', user_round: 3, status: 'completed', content: 'Fixture result input' });
    apply(8, 'item_upsert', { turn_id: 'result-turn', item_id: 'result-turn:text-0', kind: 'assistant_message',
      role: 'assistant', model_round: 0, revision: 1, status: 'completed', content: 'Fixture isolated result' });
    assert.equal(chat.isSessionBusy(key), false);
    assert.deepEqual(buildChatThreadMaterializedMessages(key)!.filter(row => row.role === 'assistant')
      .map(row => row.status), ['cancelled', 'failed', 'final']);
  } finally { cleanup(); }
});

test('scheduled preemption projects the new root turn live and emits one completion', async () => {
  const { chat, events, eventDetails, apply, cleanup } = await setup();
  try {
    apply(1, 'turn_upsert', { turn_id: 'old-turn', user_round: 1, status: 'running' });
    apply(2, 'item_upsert', { turn_id: 'old-turn', item_id: 'old-turn:user', kind: 'user_message',
      role: 'user', content: 'interactive request', user_round: 1, revision: 1 });
    apply(3, 'item_upsert', { turn_id: 'old-turn', item_id: 'old-turn:text-1', kind: 'assistant_message',
      role: 'assistant', content: 'partial output', model_round: 1, status: 'running', revision: 1 });
    apply(4, 'turn_status', { turn_id: 'old-turn', status: 'cancelled' });
    apply(5, 'turn_upsert', { turn_id: 'scheduled-turn', user_round: 2, status: 'queued' });
    apply(6, 'item_upsert', { turn_id: 'scheduled-turn', item_id: 'scheduled-turn:user', kind: 'user_message',
      role: 'user', content: 'scheduled delivery', user_round: 2, revision: 1 });
    apply(7, 'item_upsert', { turn_id: 'scheduled-turn', item_id: 'scheduled-turn:queue', kind: 'queue',
      status: 'queued', queue_ahead: 0, revision: 1 });
    apply(8, 'turn_status', { turn_id: 'scheduled-turn', status: 'running' });
    apply(9, 'item_upsert', { turn_id: 'scheduled-turn', item_id: 'scheduled-turn:tool', kind: 'tool_call',
      event_type: 'tool_result', tool: 'schedule_task', status: 'completed', revision: 1,
      meta: { duration_ms: 12 } });
    apply(10, 'item_upsert', { turn_id: 'scheduled-turn', item_id: 'scheduled-turn:text-1', kind: 'assistant_message',
      role: 'assistant', content: 'scheduled result', model_round: 1, status: 'completed', revision: 1 });
    apply(11, 'turn_upsert', { turn_id: 'scheduled-turn', status: 'completed' });
    apply(12, 'turn_status', { turn_id: 'scheduled-turn', status: 'completed' });

    const rows = buildChatThreadMaterializedMessages(key)!;
    assert.deepEqual(rows.map(row => row.role), ['user', 'assistant', 'user', 'assistant']);
    assert.equal(rows[1].status, 'cancelled');
    assert.equal(rows[1].content, 'partial output');
    assert.equal(rows[3].status, 'final');
    assert.equal(rows[3].content, 'scheduled result');
    assert.equal(rows[3].workflowItems?.length, 2); // queue + tool result
    const completions = eventDetails
      .filter(detail => Array.isArray(detail?.completedTurns))
      .flatMap(detail => detail.completedTurns);
    assert.equal(completions.length, 1);
    assert.equal(completions[0].turnId, 'scheduled-turn');
  } finally { cleanup(); }
});
