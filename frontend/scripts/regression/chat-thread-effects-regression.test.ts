import './mocks/browserStorage';
import test from 'node:test';
import assert from 'node:assert/strict';
import { createPinia, setActivePinia } from 'pinia';
import { applyChatThreadServerEvent, submitRawChatThreadTurn, hydrateChatThreadRuntime, resetChatThreadRuntime, buildChatThreadMaterializedMessages } from '../../src/realtime/chat/chatThreadRuntime';
import {beginThreadView,commitThreadView,resetThreadViews, type ChatViewRow} from '../../src/realtime/chat/threadViewStore';
import { buildWorkflowToolRuns } from '../../src/components/chat/toolWorkflowRunModel';
import { resolveCollapsedWorkflowEntryMetadata } from '../../src/components/chat/toolWorkflowCollapsedMetadata';

const key = 'fixture-thread-effects';
const setup = async () => {
  Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: {
    getItem: () => null, setItem: () => {}, removeItem: () => {}
  } });
  const { useChatStore } = await import('../../src/stores/chat');
  const { applyChatThreadEffects, applyThreadViewEffects, syncChatThreadShell } = await import('../../src/stores/chatThreadEffects');
  setActivePinia(createPinia());
  resetChatThreadRuntime(key);resetThreadViews();
  const chat = useChatStore();
  chat.activeSessionId = key;
  chat.sessions = [{ id: key, agent_id: 'fixture-agent' }] as any;
  const events: string[] = [];
  const eventDetails: any[] = [];
  const original = globalThis.window;
  globalThis.window = { dispatchEvent: (event: Event) => { events.push(event.type); eventDetails.push((event as CustomEvent).detail); return true; } } as any;
  const apply = (seq: number, change_type: string, data: Record<string, unknown>) =>
    applyChatThreadServerEvent(chat, key, 'thread_change', {
      cursor: seq, change_type, turn_id: data.turn_id, item_id: data.item_id, revision: data.revision, payload: data
    }, { onChangesApplied: changes => applyChatThreadEffects(chat, key, changes) });
  const applyView = (view_seq: number, rows: ChatViewRow[], runtime_status?: string) =>
    applyChatThreadServerEvent(chat, key, 'thread_view_change', { epoch: 1, view_seq, rows, runtime_status }, {
      onViewApplied: prior => applyThreadViewEffects(chat, key, prior)
    });
  return { chat, events, eventDetails, apply, applyView, syncChatThreadShell, cleanup: () => { globalThis.window = original; } };
};

test('durable lifecycle settles shell and dispatches workspace effects once even through replay and gaps', async () => {
  const { chat, events, apply, syncChatThreadShell, cleanup } = await setup();
  try {
    submitRawChatThreadTurn(chat, key, 'client-one', 'question');
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
    assert.equal(events.filter(event => event === 'wunder:workspace-refresh').length, 2);
    submitRawChatThreadTurn(chat, key, 'client-two', 'next question');
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
  const { chat, events, eventDetails, cleanup } = await setup();
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
    options.onEvent('thread_view_change',JSON.stringify({epoch:1,view_seq:5,rows:[{
      root_turn_id:'fixture-turn-send',execution_turn_id:'fixture-turn-send',user_round:1,client_message_id:clientId,revision:5,status:'completed',
      user:{item_id:'fixture-turn-send:user',content:'question'},assistant:{item_id:'fixture-turn-send:text-1',content:'answer'},workflow_count:0,failed_count:0
    }]}));
    assert.equal(chat.isSessionBusy(key), false);
    assert.equal(chat.isSessionLoading(key), false);
    assert.deepEqual(eventDetails.flatMap(detail => detail?.completedTurns ?? []).map(item => item.turnId), ['fixture-turn-send']);
    const refreshes = events.filter(type => type === 'wunder:workspace-refresh').length;
    assert.equal(refreshes, 1, 'binding the pending row directly to a terminal row refreshes directories');
    options.onEvent('workspace_update', JSON.stringify({ data: { paths: ['reports/result.txt'] } }));
    assert.equal(events.filter(type => type === 'wunder:workspace-refresh').length, refreshes + 1);
    assert.deepEqual(eventDetails.at(-1).paths, ['reports/result.txt']);
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

test('view lifecycle refreshes on interaction, completion, failure and stop without replay refresh storms', async () => {
  const { events, eventDetails, applyView, cleanup } = await setup();
  const row = (status: string, revision: number): ChatViewRow => ({
    root_turn_id: 'fixture-view-turn', user_round: 1, revision, status,
    user: { item_id: 'fixture-user', content: 'question' },
    assistant: { item_id: 'fixture-answer', content: 'output' }, workflow_count: 0, failed_count: 0
  });
  try {
    applyView(1, [row('running', 1)]);
    applyView(2, [row('waiting_input', 2)]);
    applyView(3, [row('waiting_input', 3)]);
    applyView(4, [row('running', 4)]);
    applyView(5, [row('waiting_approval', 5)]);
    applyView(6, [row('running', 6)]);
    applyView(7, [row('completed', 7)]);
    applyView(8, [row('completed', 8)]);
    applyView(9, [row('running', 9)]);
    applyView(10, [row('failed', 10)]);
    applyView(11, [row('running', 11)]);
    applyView(12, [row('cancelled', 12)]);
    const refreshes = eventDetails.filter((_, index) => events[index] === 'wunder:workspace-refresh');
    assert.deepEqual(refreshes.map(detail => detail.reason), [
      'thread_waiting_input', 'thread_waiting_approval', 'thread_completed', 'thread_failed', 'thread_cancelled'
    ]);
    assert.ok(refreshes.every(detail => detail.forceTree && detail.scope === 'directory'));
    assert.equal(applyChatThreadServerEvent({}, key, 'workspace_update', {}), false);
  } finally { cleanup(); }
});

test('sending again after completion ignores old completion and settles the new turn once', async () => {
  const { chat, eventDetails, applyView, cleanup } = await setup();
  const { chatWsClient } = await import('../../src/stores/chatWatcher');
  const { clearSessionWatcher } = await import('../../src/stores/chatRuntimeControls');
  const originalRequest = chatWsClient.request;
  const previous: ChatViewRow = {
    root_turn_id: 'fixture-previous-turn', user_round: 1, revision: 10, status: 'completed',
    user: { item_id: 'fixture-previous-user', content: 'Fixture input' },
    assistant: { item_id: 'fixture-previous-answer', content: 'Fixture answer' }, workflow_count: 0, failed_count: 0
  };
  const completions = () => eventDetails.flatMap(detail => detail?.completedTurns ?? []);
  applyView(10, [previous], 'completed');
  chatWsClient.request = (async (options: any) => {
    const request = typeof options.message === 'function' ? options.message() : options.message;
    if (request.type !== 'start') {
      return new Promise(resolve => options.signal?.addEventListener('abort', () => resolve(undefined), { once: true }));
    }
    assert.equal(chat.sessionRuntimeStatus(key), 'running');
    assert.equal(chat.isSessionBusy(key), true);
    options.onEvent('thread_view_change', JSON.stringify({epoch:1,view_seq:10,runtime_status:'completed',rows:[previous]}));
    assert.equal(chat.sessionRuntimeStatus(key), 'running');
    assert.equal(completions().length, 0);
    const current: ChatViewRow = {
      ...previous, root_turn_id: 'fixture-followup-turn', user_round: 2, revision: 11,
      client_message_id: request.payload.client_message_id, status: 'running',
      assistant: { item_id: 'fixture-followup-answer', content: '' }
    };
    options.onEvent('thread_view_change', JSON.stringify({epoch:1,view_seq:11,runtime_status:'running',rows:[current]}));
    assert.equal(chat.isSessionBusy(key), true);
    assert.equal(completions().length, 0);
    const finished = { ...current, revision:12, status:'completed', assistant:{...current.assistant,content:'Fixture follow-up answer'} };
    const terminal = JSON.stringify({epoch:1,view_seq:12,runtime_status:'completed',rows:[finished]});
    options.onEvent('thread_view_change', terminal);
    options.onEvent('thread_view_change', terminal);
    assert.equal(chat.isSessionBusy(key), false);
    assert.equal(chat.isSessionLoading(key), false);
    assert.deepEqual(completions().map(item => item.turnId), ['fixture-followup-turn']);
    options.onEvent('final', JSON.stringify({content:'Fixture follow-up answer'}));
  }) as any;
  try {
    await chat.sendMessage('Fixture follow-up');
    assert.equal(chat.sessionRuntimeStatus(key), 'completed');
  } finally {
    clearSessionWatcher();
    chatWsClient.request = originalRequest;
    cleanup();
  }
});

test('durable file writes refresh while running and terminal heads clear all shell busy state', async () => {
  const { chat, events, applyView, cleanup } = await setup();
  const { ensureRuntime } = await import('../../src/stores/chatRuntimeState');
  const row = (revision: number, workspace_change_seq: number, status = 'running'): ChatViewRow => ({
    root_turn_id: 'fixture-write-turn', user_round: 1, revision, status, workspace_change_seq,
    user: { item_id: 'fixture-user', content: 'input' },
    assistant: { item_id: 'fixture-answer', content: 'output' }, workflow_count: 1, failed_count: 0
  });
  try {
    applyView(1, [row(1, 0)], 'running');
    applyView(2, [row(2, 10)], 'running');
    assert.equal(chat.isSessionBusy(key), true);
    const count = () => events.filter(event => event === 'wunder:workspace-refresh').length;
    assert.equal(count(), 1, 'queue-owned executions need no transient workspace event');
    applyView(2, [row(2, 10)], 'running');
    applyView(3, [row(3, 10)], 'running');
    assert.equal(count(), 1, 'replay and token-only changes do not rescan directories');
    applyView(4, [row(4, 20)], 'running');
    assert.equal(count(), 2);
    const runtime = ensureRuntime(key);
    runtime.pendingApprovalIds = ['fixture-approval'];
    runtime.pendingApprovalCount = 1;
    runtime.waitingForUserInput = true;
    applyView(5, [row(5, 20, 'completed')], 'completed');
    assert.equal(chat.isSessionBusy(key), false);
    assert.equal(chat.isSessionLoading(key), false);
    assert.equal(runtime.pendingApprovalCount, 0);
    assert.equal(runtime.waitingForUserInput, false);
    assert.equal(count(), 3);
  } finally { cleanup(); }
});

test('durable failure and cancellation refresh once per status boundary', async () => {
  const { events, eventDetails, apply, cleanup } = await setup();
  try {
    apply(1, 'turn_upsert', { turn_id: 'fixture-failed', status: 'running' });
    apply(2, 'turn_status', { turn_id: 'fixture-failed', status: 'waiting_approval' });
    apply(3, 'turn_status', { turn_id: 'fixture-failed', status: 'running' });
    apply(4, 'turn_status', { turn_id: 'fixture-failed', status: 'failed' });
    apply(5, 'turn_status', { turn_id: 'fixture-failed', status: 'failed' });
    apply(6, 'turn_upsert', { turn_id: 'fixture-stopped', status: 'running', user_round: 2 });
    apply(7, 'turn_status', { turn_id: 'fixture-stopped', status: 'cancelled' });
    assert.deepEqual(eventDetails.filter((_, index) => events[index] === 'wunder:workspace-refresh')
      .map(detail => detail.reason), ['thread_waiting_approval', 'thread_failed', 'thread_cancelled']);
  } finally { cleanup(); }
});

test('recovering a completed snapshot refreshes an active thread but opening idle history does not', async () => {
  const { chat, events, syncChatThreadShell, cleanup } = await setup();
  const { ensureRuntime } = await import('../../src/stores/chatRuntimeState');
  ensureRuntime(key).threadStatus = 'not_loaded';
  const row = (status: string, revision: number): ChatViewRow => ({
    root_turn_id: 'fixture-recovery', user_round: 1, revision, status,
    user: { item_id: 'fixture-user', content: 'question' },
    assistant: { item_id: 'fixture-answer', content: 'output' }, workflow_count: 0, failed_count: 0
  });
  const commit = (status: string, revision: number) => commitThreadView(key, {
    session_id: key, epoch: 1, view_seq: revision, projection_state: 'ready', thread_empty: false,
    latest_round: 1, chat: [row(status, revision)], trajectory: [], tiles: [], tile_level: 0,
    next_before: null, has_more: false
  });
  try {
    commit('completed', 1); syncChatThreadShell(chat, key);
    assert.equal(events.filter(type => type === 'wunder:workspace-refresh').length, 0);
    commit('running', 2); syncChatThreadShell(chat, key);
    commit('completed', 3); syncChatThreadShell(chat, key); syncChatThreadShell(chat, key);
    assert.equal(events.filter(type => type === 'wunder:workspace-refresh').length, 1);
  } finally { cleanup(); }
});

test('runtime close and user stop provide a forced directory refresh fallback', async () => {
  const { chat, events, eventDetails, cleanup } = await setup();
  const { ensureRuntime, applySessionRuntimeEvent, settleUserStoppedSessionRuntime } =
    await import('../../src/stores/chatRuntimeState');
  try {
    ensureRuntime(key).threadStatus = 'running';
    applySessionRuntimeEvent(chat, key, {}, 'thread_closed');
    ensureRuntime(key).threadStatus = 'running';
    settleUserStoppedSessionRuntime(chat, key);
    const refreshes = eventDetails.filter((_, index) => events[index] === 'wunder:workspace-refresh');
    assert.deepEqual(refreshes.map(detail => detail.reason), ['thread_not_loaded', 'thread_cancelled']);
    assert.ok(refreshes.every(detail => detail.forceTree));
  } finally { cleanup(); }
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
    assert.equal(completions.filter(c=>c.turnId==='scheduled-turn').length, 1);
    assert.equal(completions.find(c=>c.turnId==='old-turn')?.status, 'cancelled');
  } finally { cleanup(); }
});
