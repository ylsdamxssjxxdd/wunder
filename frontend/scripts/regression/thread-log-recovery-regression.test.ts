import test from 'node:test';
import assert from 'node:assert/strict';
import { createChatRuntimeProjection, applyChatRuntimeEvent } from '../../src/realtime/chat/chatRuntimeReducer';
import { buildCanonicalChatRuntimeEvents } from '../../src/realtime/chat/chatCanonicalEvents';
import { buildCanonicalSessionEventsSnapshot } from '../../src/realtime/chat/chatRuntimeBridge';
import { selectSessionBusy, selectVisibleMessageProjections } from '../../src/realtime/chat/chatRuntimeSelectors';
import { threadLogCursor, advanceThreadLogCursor } from '../../src/stores/chatThreadCursor';
import { buildWorkflowToolRuns } from '../../src/components/chat/toolWorkflowRunModel';
import { resolveCollapsedWorkflowEntryMetadata } from '../../src/components/chat/toolWorkflowCollapsedMetadata';
import { createPinia, setActivePinia } from 'pinia';

const assistant = (status = 'streaming', content = 'prefix😀') => ({
  message_id: 'item:answer', item_id: 'answer', role: 'assistant', status, content,
  user_turn_id: 'user-turn:thread:round:1',
  model_turn_id: 'model-turn:thread:user:1:model:1',
  user_turn_index: 1, model_turn_index: 1, turn_index: 2
});

test('refresh restores both text fields then consumes independent changes without losing prefix', () => {
  const projection = createChatRuntimeProjection();
  const runtime = { threadLogCursor: 20, lastEventId: 900 };
  applyChatRuntimeEvent(projection, {event_type:'session_snapshot',source:'snapshot',strict:false,
    session_id:'thread',messages:[assistant()],payload:{runtime_status:'running'}});
  const emitBlock = (field: string, text: string, cursor: number) => {
    const events = buildCanonicalChatRuntimeEvents({sessionId:'thread',eventType:'thread_item_block',
      payload:{data:{item_id:'answer',message_id:'item:answer',user_round:1,model_round:1,
        block_index:1,field,cursor,event_seq:900,[field]:text,
        [`${field}_offset`]:field === 'content' ? 'prefix😀'.length : 0}}});
    events.forEach(event => applyChatRuntimeEvent(projection, event));
    advanceThreadLogCursor(runtime, cursor);
    return events;
  };
  const events = emitBlock('content', ' tail', 21);
  emitBlock('reasoning', 'thought', 22);
  events.forEach(event => applyChatRuntimeEvent(projection, event));
  let message = selectVisibleMessageProjections(projection, 'thread')[0];
  assert.equal(message.content, 'prefix😀 tail');
  assert.equal(message.reasoning, 'thought');
  assert.equal(threadLogCursor(runtime), 22);
  assert.equal(projection.sessions.thread.lastAppliedEventId, 0);
  assert.equal(projection.sessions.thread.appliedSeq < 900, true);
  assert.deepEqual(buildCanonicalChatRuntimeEvents({sessionId:'thread',eventType:'thread_change',
    eventId:901,payload:{cursor:23}}), []);
  applyChatRuntimeEvent(projection, {event_type:'session_snapshot',source:'snapshot',strict:false,
    session_id:'thread',messages:[assistant('final', 'prefix😀 tail')],payload:{runtime_status:'idle'}});
  emitBlock('content', ' late', 24);
  message = selectVisibleMessageProjections(projection, 'thread')[0];
  assert.equal(message.content, 'prefix😀 tail');
  assert.equal(message.status, 'final');
  assert.equal(selectSessionBusy(projection, 'thread'), false);
});

test('runtime recovery can change status repeatedly at the same durable cursor', () => {
  const projection = createChatRuntimeProjection();
  for (const status of ['running', 'idle', 'running', 'idle']) {
    buildCanonicalChatRuntimeEvents({sessionId:'thread',eventType:'thread_status',
      payload:{data:{thread_status:status,recovery:true,cursor:7}}})
      .forEach(event => applyChatRuntimeEvent(projection,event));
    assert.equal(selectSessionBusy(projection,'thread'), status === 'running');
  }
});

test('watch resumes from the durable ThreadLog cursor; transport cursor never enters the payload or the legacy reducer', async () => {
  const values = new Map<string, string>();
  Object.defineProperty(globalThis, 'localStorage', {configurable:true,value:{
    getItem:(key:string) => values.get(key) ?? null,
    setItem:(key:string,value:string) => values.set(key,value), removeItem:(key:string) => values.delete(key)
  }});
  setActivePinia(createPinia());
  const { useChatStore } = await import('../../src/stores/chat');
  const { startSessionWatcher, chatWsClient } = await import('../../src/stores/chatWatcher');
  const { ensureRuntime, syncChatRuntimeProjectionFromSnapshot, cacheSessionMessages } =
    await import('../../src/stores/chatRuntimeState');
  const { applyChatThreadServerEvent, getChatThreadState } =
    await import('../../src/realtime/chat/chatThreadRuntime');
  const store = useChatStore();
  store.resetState();
  store.activeSessionId = 'thread';
  store.sessions = [{id:'thread'}];
  store.messages = [{...assistant(),turn_id:'turn'}];
  cacheSessionMessages('thread', store.messages);
  const runtime = ensureRuntime('thread');
  runtime.threadStatus = 'running';
  runtime.threadLogCursor = 20;
  runtime.lastEventId = 900;
  syncChatRuntimeProjectionFromSnapshot(store, 'thread', store.messages, {running:true});
  assert.equal(
    selectVisibleMessageProjections(store.runtimeProjection,'thread')[0]?.content ?? null,
    'prefix😀'
  );
  const legacyContentBefore =
    selectVisibleMessageProjections(store.runtimeProjection,'thread')[0]?.content ?? null;
  // Seed the durable reducer: its ThreadLog cursor (lastSeq) must differ from
  // the transport event cursor (runtime.lastEventId) and the snapshot cursor
  // (runtime.threadLogCursor) so any leak would be observable.
  const seedFrames: Array<[string, Record<string, unknown>]> = [];
  for (let seq = 1; seq <= 5; seq += 1) {
    const wire = seq === 5
      ? { event: 'thread_change', data: {
          change_type: 'turn_upsert', turn_id: 'turn', cursor: seq, revision: 1,
          payload: { turn_id: 'turn', status: 'completed', user_round: 1 } } }
      : { event: 'thread_change', data: {
          change_type: 'item_upsert', turn_id: 'turn', cursor: seq, revision: 1,
          item: {
            item_id: `turn:item-${seq}`, turn_id: 'turn', kind: 'assistant_message',
            status: seq === 1 ? 'running' : 'completed', revision: 1, visibility: 'user',
            payload: { role: 'assistant', user_round: 1, model_round: 1, content: '', reasoning: '' }
          } } };
    seedFrames.push(['thread_change', wire]);
    applyChatThreadServerEvent(store, 'thread', 'thread_change', wire);
  }
  applyChatThreadServerEvent(store, 'thread', 'thread_item_tail', {
    event: 'thread_item_tail',
    data: { item_id: 'turn:item-1', field: 'content', offset: 0, text: '你好' }
  });
  assert.equal(getChatThreadState('thread')?.lastSeq, 5);
  // Durable frames write only the durable reducer; the legacy projection keeps
  // its snapshot content and the durable tail lands in the durable tails map.
  assert.equal(
    selectVisibleMessageProjections(store.runtimeProjection,'thread')[0]?.content ?? null,
    legacyContentBefore
  );
  assert.equal(getChatThreadState('thread')?.tails.get('turn:item-1')?.content, '你好');

  const originalRequest = chatWsClient.request;
  let watch: any;
  chatWsClient.request = options => {
    watch = options;
    return new Promise<void>((_resolve, reject) => options.signal.addEventListener('abort', () =>
      reject(Object.assign(new Error('aborted'), {name:'AbortError'})), {once:true}));
  };
  try {
    startSessionWatcher(store, 'thread');
    // The watch resumes from the durable ThreadLog cursor; the transport event
    // cursor (900) and the snapshot cursor (20) never enter the payload, and
    // the send path consumes neither clock.
    const wire = watch.message();
    assert.equal(wire.type, 'watch');
    assert.equal(wire.payload.after_change_seq, 5);
    assert.equal(wire.payload.after_event_id, undefined);
    assert.equal(runtime.lastEventId, 900);
    assert.equal(runtime.threadLogCursor, 20);

    // Replayed and stale frames are idempotent: the durable state does not move
    // and no stale content is written.
    seedFrames.forEach(([type, payload]) =>
      watch.onEvent(type, JSON.stringify(payload), null));
    watch.onEvent('thread_change', JSON.stringify({ event: 'thread_change', data: {
      change_type: 'item_upsert', turn_id: 'turn', cursor: 2, revision: 9,
      item: { item_id: 'turn:item-2', turn_id: 'turn', kind: 'assistant_message',
        status: 'completed', revision: 9, visibility: 'user',
        payload: { role: 'assistant', content: '篡改' } } } }), null);
    assert.equal(getChatThreadState('thread')?.lastSeq, 5);
    assert.equal(getChatThreadState('thread')?.items.get('turn:item-2')?.content, '');
    assert.equal(getChatThreadState('thread')?.tails.get('turn:item-1')?.content, '你好');
    assert.equal(
      selectVisibleMessageProjections(store.runtimeProjection,'thread')[0]?.content ?? null,
      legacyContentBefore
    );
  } finally {
    store.resetState();
    chatWsClient.request = originalRequest;
  }
});

test('idle snapshot overrides stale workflow flags and historical tools keep metrics without reviving a bubble', () => {
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(projection, {event_type:'session_snapshot',source:'snapshot',strict:false,
    session_id:'thread',messages:[{...assistant('final', 'answer'),workflowStreaming:true,
      subagents:[{key:'child',status:'running',terminal:false}]}],payload:{runtime_status:'idle'}});
  const history = buildCanonicalSessionEventsSnapshot({sessionId:'thread',payload:{rounds:[{
    user_round:1,events:[{item_id:'tool',revision:2,event:'tool_result',data:{
      user_round:1,model_round:1,tool:'read_file',tool_call_id:'call',ok:true,
      request_context_tokens:120,request_usage:{input_tokens:120},meta:{duration_ms:1250}
    }}]
  }]}});
  history.forEach(event => applyChatRuntimeEvent(projection, event));
  const message = selectVisibleMessageProjections(projection, 'thread')[0];
  assert.equal(message.status, 'final');
  assert.equal(selectSessionBusy(projection, 'thread'), false);
  const rows = buildWorkflowToolRuns(message.workflowItems || []);
  assert.equal(rows.length, 1);
  const labels = resolveCollapsedWorkflowEntryMetadata(rows[0]);
  assert.equal(labels.contextTokensLabel, '120 token');
  assert.equal(labels.durationLabel, '1.3s');
});

test('durable item envelope keeps automatic compaction completed without creating a manual marker', () => {
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(projection, {event_type:'session_snapshot',source:'snapshot',strict:false,
    session_id:'thread',messages:[assistant('final', 'answer')],payload:{runtime_status:'idle'}});
  const history = buildCanonicalSessionEventsSnapshot({sessionId:'thread',payload:{rounds:[{
    user_round:1,events:[{item_id:'compact',revision:2,event:'compaction',data:{
      user_round:1,model_round:1,status:'done',trigger_mode:'auto',compaction_id:'compact-1'
    }}]
  }]}});
  history.forEach(event => applyChatRuntimeEvent(projection, event));
  const message = selectVisibleMessageProjections(projection, 'thread')[0];
  assert.equal(message.status, 'final');
  assert.equal(message.workflowItems?.[0]?.status, 'completed');
  assert.notEqual(message.display?.manual_compaction_marker, true);
  assert.equal(selectSessionBusy(projection, 'thread'), false);
});

test('tool recovery keeps the outer call identity and arguments beside nested result data', () => {
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(projection, {event_type:'session_snapshot',source:'snapshot',strict:false,
    session_id:'thread',messages:[assistant('final', 'answer')],payload:{runtime_status:'idle'}});
  const history = buildCanonicalSessionEventsSnapshot({sessionId:'thread',payload:{rounds:[{
    user_round:1,events:[{item_id:'tool',revision:3,event:'tool_result',data:{
      user_round:1,model_round:1,tool:'read_file',tool_call_id:'call-1',
      args:{path:'sample.txt'},data:{content:'file body'},status:'completed'
    }}]
  }]}});
  history.forEach(event => applyChatRuntimeEvent(projection, event));
  const item = selectVisibleMessageProjections(projection, 'thread')[0].workflowItems?.[0];
  assert.equal(item?.toolName, 'read_file');
  assert.equal(item?.toolCallId, 'call-1');
  assert.match(String(item?.toolCallRawDetail), /sample\.txt/);
  assert.match(String(item?.toolResultRawDetail), /file body/);
});

test('item indexes in different rounds never share transport deduplication', () => {
  const events = buildCanonicalSessionEventsSnapshot({sessionId:'thread',payload:{rounds:[1,2].map(round => ({
    user_round:round,events:[{event:'tool_result',item_id:`tool-${round}`,revision:2,item_index:1,
      data:{user_round:round,model_round:1,tool:'read_file',tool_call_id:`call-${round}`,ok:true}}]
  }))}});
  assert.equal(events.length, 2);
  assert.notEqual(events[0].event_id, events[1].event_id);
  assert.ok(events.every(event => event.event_seq === null));
  const first = {threadLogCursor:5};
  const second = {threadLogCursor:0};
  advanceThreadLogCursor(first, 7);
  advanceThreadLogCursor(first, 3);
  assert.equal(threadLogCursor(first), 7);
  assert.equal(threadLogCursor(second), 0);
});

test('snapshot rebuild clears bounded gap and tail temp state and binds the durable cursor', async () => {
  const values = new Map<string, string>();
  Object.defineProperty(globalThis, 'localStorage', {configurable:true,value:{
    getItem:(key:string) => values.get(key) ?? null,
    setItem:(key:string,value:string) => values.set(key,value), removeItem:(key:string) => values.delete(key)
  }});
  setActivePinia(createPinia());
  const { useChatStore } = await import('../../src/stores/chat');
  const { applyChatThreadServerEvent, ensureChatThreadRuntime, getChatThreadState,
    registerChatThreadSnapshotLoader, resetChatThreadRuntime } =
    await import('../../src/realtime/chat/chatThreadRuntime');
  resetChatThreadRuntime('thread');
  ensureChatThreadRuntime('thread');
  registerChatThreadSnapshotLoader(async () => ({
    cursor: 40,
    turns: [{ turn_id: 'turn-r', user_round: 1, status: 'running' }],
    items: [{
      item_id: 'turn-r:text-1', turn_id: 'turn-r', kind: 'assistant_message',
      status: 'running', revision: 1, visibility: 'user',
      payload: { role: 'assistant', content: '重建文本', reasoning: '' }
    }],
    blocks: []
  }));
  const store = useChatStore();
  store.resetState();
  store.activeSessionId = 'thread';
  store.sessions = [{id:'thread'}];

  // Seed contiguous state, then buffer a far-ahead frame in the gap and a tail
  // on the active item: both are temporary state the atomic rebuild must drop.
  for (let seq = 1; seq <= 30; seq += 1) {
    applyChatThreadServerEvent(store, 'thread', 'thread_change', {
      event: 'thread_change', data: { change_type: 'turn_upsert', turn_id: 'turn-g', cursor: seq,
        revision: 1, payload: { turn_id: 'turn-g', status: 'running', user_round: 1 } } });
  }
  applyChatThreadServerEvent(store, 'thread', 'thread_item_tail', {
    event: 'thread_item_tail', data: { item_id: 'turn-g:text-1', field: 'content', offset: 0, text: '旧尾' } });
  applyChatThreadServerEvent(store, 'thread', 'thread_change', {
    event: 'thread_change', data: { change_type: 'turn_upsert', turn_id: 'turn-far', cursor: 99,
      revision: 1, payload: { turn_id: 'turn-far', status: 'running' } } });
  assert.equal(getChatThreadState('thread')?.lastSeq, 30);
  assert.equal(getChatThreadState('thread')?.gap.length, 1);
  assert.equal(getChatThreadState('thread')?.tails.get('turn-g:text-1')?.content, '旧尾');

  let snapshotApplied = 0;
  applyChatThreadServerEvent(store, 'thread', 'thread_snapshot_required', {
    event: 'thread_snapshot_required', data: { required_from_seq: 31, earliest_available_seq: 40 }
  }, { onSnapshotApplied: () => { snapshotApplied += 1; } });
  await new Promise(resolve => setTimeout(resolve, 10));

  assert.equal(snapshotApplied, 1);
  const state = getChatThreadState('thread')!;
  assert.equal(state.lastSeq, 40);
  assert.equal(state.gap.length, 0);
  assert.equal(state.tails.get('turn-g:text-1'), undefined);
  // The rebuild bound the snapshot cursor; old buffered frames are gone.
  assert.equal(state.turns.has('turn-g'), false);
  assert.equal(state.turns.get('turn-r')?.status, 'running');
  registerChatThreadSnapshotLoader(null);
});
