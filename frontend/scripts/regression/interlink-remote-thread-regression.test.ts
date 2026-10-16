import test from 'node:test';
import assert from 'node:assert/strict';
import { remoteThreadModel, remoteHistoryText } from '../../src/views/messenger/interlink/remoteThreadModel';

test('remote history renders text and keeps markup inert', () => {
  assert.equal(remoteHistoryText([{role:'user',content:'<sample>'},{role:'assistant',content:'reply'}]), '› <sample>\n\n◆ reply');
});
test('remote subscription fences another thread and requests overflow recovery', () => {
  const model = remoteThreadModel('thread_fixture');
  assert.equal(model.apply({type:'delta',thread_id:'other',payload:{overflow:true}}), false);
  assert.equal(model.apply({type:'delta',thread_id:'thread_fixture',payload:{overflow:true}}), true);
});
test('snapshot replay keeps text stable and excludes internal content', () => {
  const model = remoteThreadModel('thread_fixture');
  const frame = {type:'snapshot',thread_id:'thread_fixture',payload:{cursor:1,turns:[],blocks:[],items:[
    {item_id:'item_fixture',turn_id:'turn_fixture',kind:'assistant_message',status:'completed',revision:1,payload:{content:'reply',role:'assistant'}},
    {item_id:'internal_fixture',turn_id:'turn_fixture',kind:'assistant_message',visibility:'admin',status:'completed',revision:1,payload:{content:'hidden',visibility:'admin'}}
  ]}};
  model.apply(frame); model.apply(frame);
  assert.equal(model.text(), '◆ reply');
});
