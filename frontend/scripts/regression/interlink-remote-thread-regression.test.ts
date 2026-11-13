import test from 'node:test';
import assert from 'node:assert/strict';
import { remoteThreadModel, remoteHistoryItems } from '../../src/views/messenger/interlink/remoteThreadModel';

// 远端历史只做数据规整（角色 / 身份 / 截断），标记由渲染层当纯文本插值，
// 所以 '<sample>' 必须原样留在 content 里，不能在这里被当 HTML 处理。
test('remote history normalizes rows and keeps markup inert', () => {
  assert.deepEqual(remoteHistoryItems([{ role: 'user', content: '<sample>' }, { role: 'assistant', content: 'reply' }]), [
    { id: 'user:0', role: 'user', content: '<sample>' },
    { id: 'assistant:1', role: 'assistant', content: 'reply' }
  ]);
  // 空内容行不产生可见气泡，与渲染层「不画空行」一致。
  assert.deepEqual(remoteHistoryItems([{ role: 'user', content: '   ' }, { role: 'assistant', content: '' }]), []);
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
