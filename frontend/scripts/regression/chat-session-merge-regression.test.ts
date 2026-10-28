import test from 'node:test';
import assert from 'node:assert/strict';

import { mergeSessionsByIdPreservingRuntimeFields } from '../../src/stores/chatSessionMerge';
import { applySessionQuotaUsage } from '../../src/stores/chatSessionQuota';
import { beginThreadView,commitThreadView,threadViewConfirmsCompletion,clearThreadView,threadViewSlots } from '../../src/realtime/chat/threadViewStore';

test('catalog completion at an unchanged history cursor settles its bubble and rejects a premature notice',()=>{
  const key='fixture-catalog-thread';
  const generation=beginThreadView(key);
  const row={root_turn_id:'fixture-catalog-turn',execution_turn_id:'fixture-catalog-turn',user_round:1,revision:5,status:'running',
    user:{item_id:'fixture-user-item',content:'Fixture input'},assistant:{item_id:'',content:''},workflow_count:1,failed_count:0};
  commitThreadView(key,{session_id:key,epoch:1,view_seq:5,projection_state:'ready',thread_empty:false,latest_round:1,chat:[row],trajectory:[],tiles:[],tile_level:0,next_before:1,has_more:false,
    runtime_status:'running',status_turn_id:'fixture-catalog-turn',active_turn_id:'fixture-catalog-turn'},generation);
  assert.equal(threadViewConfirmsCompletion(key,'fixture-catalog-turn','completed'),false);
  const merged=mergeSessionsByIdPreservingRuntimeFields([{id:key}],[{id:key,runtime_head:{epoch:1,view_seq:5,runtime_status:'completed',status_turn_id:'fixture-catalog-turn',active_turn_id:''}}],session=>({...session}),items=>items);
  assert.equal(merged[0].runtime_status,'completed');
  assert.equal(threadViewSlots(key)![0].assistant.streaming,false);
  assert.equal(threadViewConfirmsCompletion(key,'fixture-catalog-turn','completed'),true);
  clearThreadView(key);
});

test('session merge keeps locally enhanced context fields when server refresh omits them', () => {
  const currentSessions = [
    {
      id: 'sess_a',
      title: 'Current',
      contextTokens: 4583,
      contextTotalTokens: 32768,
      goal: { objective: 'persist' }
    }
  ];
  const incomingSessions = [
    {
      id: 'sess_a',
      title: 'Current',
      updated_at: '2026-05-09T10:00:00.000Z'
    }
  ];

  const merged = mergeSessionsByIdPreservingRuntimeFields(
    currentSessions,
    incomingSessions,
    (session) => ({ ...(session || {}) }),
    (sessions) => sessions
  );

  assert.equal(merged.length, 1);
  assert.equal(merged[0].contextTokens, 4583);
  assert.equal(merged[0].contextTotalTokens, 32768);
  assert.deepEqual(merged[0].goal, { objective: 'persist' });
  assert.equal(merged[0].updated_at, '2026-05-09T10:00:00.000Z');
});

test('session request count survives live model request usage and later stale list refresh', () => {
  const sessions = [{ id: 'sess_a', model_request_count: 2, quota_used: 2 }];
  assert.equal(applySessionQuotaUsage(sessions, 'sess_a', {
    session_request_count: 3,
    request_count: 1
  }), true);
  const merged = mergeSessionsByIdPreservingRuntimeFields(
    sessions,
    [{ id: 'sess_a', model_request_count: 2, quota_used: 2 }],
    (session) => ({ ...(session || {}) }),
    (items) => items
  );
  assert.equal(merged[0].model_request_count, 3);
  assert.equal(merged[0].quota_used, 3);
});
