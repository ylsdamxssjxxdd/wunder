import test from 'node:test';
import assert from 'node:assert/strict';
import { createChatRuntimeProjection, applyChatRuntimeEvent } from '../../src/realtime/chat/chatRuntimeReducer';
import { buildCanonicalChatRuntimeEvents } from '../../src/realtime/chat/chatCanonicalEvents';
import { buildCanonicalSessionEventsSnapshot } from '../../src/realtime/chat/chatRuntimeBridge';
import { selectSessionBusy, selectVisibleMessageProjections } from '../../src/realtime/chat/chatRuntimeSelectors';
import { threadChangeCursor, advanceThreadChangeCursor } from '../../src/stores/chatThreadCursor';
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
  const runtime = { threadChangeCursor: 20, lastEventId: 900 };
  applyChatRuntimeEvent(projection, {event_type:'session_snapshot',source:'snapshot',strict:false,
    session_id:'thread',messages:[assistant()],payload:{runtime_status:'running'}});
  const emitBlock = (field: string, text: string, cursor: number) => {
    const events = buildCanonicalChatRuntimeEvents({sessionId:'thread',eventType:'thread_item_block',
      payload:{data:{item_id:'answer',message_id:'item:answer',user_round:1,model_round:1,
        block_index:1,field,cursor,event_seq:900,[field]:text,
        [`${field}_offset`]:field === 'content' ? 'prefix😀'.length : 0}}});
    events.forEach(event => applyChatRuntimeEvent(projection, event));
    advanceThreadChangeCursor(runtime, cursor);
    return events;
  };
  const events = emitBlock('content', ' tail', 21);
  emitBlock('reasoning', 'thought', 22);
  events.forEach(event => applyChatRuntimeEvent(projection, event));
  let message = selectVisibleMessageProjections(projection, 'thread')[0];
  assert.equal(message.content, 'prefix😀 tail');
  assert.equal(message.reasoning, 'thought');
  assert.equal(threadChangeCursor(runtime), 22);
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

test('watch uses a durable cursor and publishes text, tool metrics and terminal Items to the visible projection', async () => {
  const values = new Map<string, string>();
  Object.defineProperty(globalThis, 'localStorage', {configurable:true,value:{
    getItem:(key:string) => values.get(key) ?? null,
    setItem:(key:string,value:string) => values.set(key,value), removeItem:(key:string) => values.delete(key)
  }});
  setActivePinia(createPinia());
  const { useChatStore } = await import('../../src/stores/chat');
  const { default: api } = await import('../../src/api/http');
  const { startSessionWatcher, chatWsClient } = await import('../../src/stores/chatWatcher');
  const { ensureRuntime, syncChatRuntimeProjectionFromSnapshot, cacheSessionMessages } =
    await import('../../src/stores/chatRuntimeState');
  const store = useChatStore();
  store.resetState();
  store.activeSessionId = 'thread';
  store.sessions = [{id:'thread'}];
  store.messages = [{...assistant(),turn_id:'turn'}];
  cacheSessionMessages('thread', store.messages);
  const runtime = ensureRuntime('thread');
  runtime.threadStatus = 'running';
  runtime.threadChangeCursor = 20;
  runtime.lastEventId = 900;
  syncChatRuntimeProjectionFromSnapshot(store, 'thread', store.messages, {running:true});
  const originalRequest = chatWsClient.request;
  const originalAdapter = api.defaults.adapter;
  let watch: any;
  let terminal = false;
  let requests = 0;
  chatWsClient.request = options => {
    watch = options;
    return new Promise<void>((_resolve, reject) => options.signal.addEventListener('abort', () =>
      reject(Object.assign(new Error('aborted'), {name:'AbortError'})), {once:true}));
  };
  api.defaults.adapter = async config => {
    requests++;
    assert.ok(config.url?.includes('/thread-log/turns/'));
    return {status:200,statusText:'OK',headers:{},config,data:{data:{turn:{
      turn_id:'turn',user_turn_index:1,status:terminal?'completed':'running',has_more:false,
      items:[{item_id:'answer',turn_id:'turn',kind:'assistant_message',status:terminal?'completed':'running',
        revision:terminal?3:2,payload:{role:'assistant',user_round:1,model_round:1,
          content:terminal?'final answer':'prefix😀 tail'}},
        {item_id:'tool',kind:'tool_call',status:'completed',revision:2,payload:{}}],
      events:[{item_id:'tool',revision:2,event:'tool_result',data:{user_round:1,model_round:1,
        tool:'read_file',tool_call_id:'call',ok:true,request_context_tokens:120,meta:{duration_ms:1250}}}]
    }}}};
  };
  const tick = () => new Promise(resolve => setTimeout(resolve, 20));
  try {
    startSessionWatcher(store, 'thread');
    assert.equal(watch.message.payload.after_event_id, 20);
    watch.onEvent('thread_item_block', JSON.stringify({data:{item_id:'answer',message_id:'item:answer',
      user_round:1,model_round:1,field:'content',block_index:1,content_offset:'prefix😀'.length,
      content:' tail',cursor:21}}), null);
    watch.onEvent('thread_change', JSON.stringify({data:{change_type:'text_block',turn_id:'turn',item_id:'answer',cursor:21}}), null);
    await tick();
    assert.equal(requests, 0);
    assert.equal(selectVisibleMessageProjections(store.runtimeProjection,'thread')[0].content, 'prefix😀 tail');
    watch.onEvent('thread_change', JSON.stringify({data:{change_type:'item_upsert',turn_id:'turn',item_id:'tool',cursor:22}}), null);
    await tick();
    const message = selectVisibleMessageProjections(store.runtimeProjection,'thread')[0];
    assert.equal(message.content, 'prefix😀 tail');
    assert.equal(resolveCollapsedWorkflowEntryMetadata(buildWorkflowToolRuns(message.workflowItems || [])[0]).durationLabel, '1.3s');
    terminal = true;
    watch.onEvent('thread_change', JSON.stringify({data:{change_type:'item_upsert',turn_id:'turn',item_id:'answer',cursor:23}}), null);
    watch.onEvent('thread_status', JSON.stringify({data:{thread_status:'idle',recovery:true,cursor:23}}), null);
    await tick();
    const final = selectVisibleMessageProjections(store.runtimeProjection,'thread')[0];
    assert.equal(final.content, 'final answer');
    assert.equal(final.status, 'final');
    assert.equal(store.isSessionBusy('thread'), false);
    assert.equal(runtime.threadChangeCursor, 23);
    assert.equal(requests, 2);
  } finally {
    store.resetState();
    chatWsClient.request = originalRequest;
    api.defaults.adapter = originalAdapter;
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

test('item indexes in different rounds never share transport deduplication', () => {
  const events = buildCanonicalSessionEventsSnapshot({sessionId:'thread',payload:{rounds:[1,2].map(round => ({
    user_round:round,events:[{event:'tool_result',item_id:`tool-${round}`,revision:2,item_index:1,
      data:{user_round:round,model_round:1,tool:'read_file',tool_call_id:`call-${round}`,ok:true}}]
  }))}});
  assert.equal(events.length, 2);
  assert.notEqual(events[0].event_id, events[1].event_id);
  assert.ok(events.every(event => event.event_seq === null));
  const first = {threadChangeCursor:5};
  const second = {threadChangeCursor:0};
  advanceThreadChangeCursor(first, 7);
  advanceThreadChangeCursor(first, 3);
  assert.equal(threadChangeCursor(first), 7);
  assert.equal(threadChangeCursor(second), 0);
});
