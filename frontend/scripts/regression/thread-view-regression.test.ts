import assert from 'node:assert/strict';
import {beginThreadView,commitThreadView,threadViewSlots,threadViewPhase,threadViewCursor,applyThreadViewChanges,submitThreadView,failThreadView,clearThreadView,applyThreadViewTail,threadViewAtLatest,threadViewStatus,pauseThreadViewHistory,threadViewHistory,threadViewWindow,removeThreadViewTurnByClientMessageId} from '../../src/realtime/chat/threadViewStore';
const row=(round:number,revision=1)=>({root_turn_id:`fixture-turn-${round}`,user_round:round,revision,status:'completed',user:{item_id:`fixture-user-${round}`,content:'fixture input'},assistant:{item_id:`fixture-answer-${round}`,content:'fixture answer'},workflow_count:0,failed_count:0});
const page=(chat:any[],seq=1)=>({session_id:'fixture-thread',epoch:1,view_seq:seq,projection_state:'ready',thread_empty:chat.length===0,latest_round:chat.length,chat,trajectory:[],tiles:[],tile_level:0,next_before:1,has_more:false});
const key='fixture-thread';
assert.deepEqual(threadViewHistory(key), { before: null, hasMore: false });
let generation=beginThreadView(key);assert.equal(threadViewPhase(key),'opening');
assert.equal(commitThreadView(key,page([row(1)]),generation),true);
const visible=threadViewSlots(key)![0];assert.equal(threadViewPhase(key),'ready');
generation=beginThreadView(key);failThreadView(key,generation,'fixture error');
assert.equal(threadViewSlots(key)![0],visible);assert.equal(threadViewPhase(key),'ready');
const stale=generation;generation=beginThreadView(key);
assert.equal(commitThreadView(key,page([]),stale),false);assert.equal(threadViewSlots(key)!.length,1);
assert.equal(commitThreadView(key,{...page([]),projection_state:'rebuilding',thread_empty:null},generation),false);
assert.equal(threadViewSlots(key)![0],visible);
applyThreadViewChanges(key,{view_seq:10,rows:[row(2,10)]});
assert.equal(commitThreadView(key,page([row(3)],9),generation),false);
commitThreadView(key,page([row(0,2)],900),undefined,true);assert.equal(threadViewCursor(key),10);
assert.equal(threadViewSlots(key)!.find(s=>s.rootTurnId==='fixture-turn-1'),visible);
pauseThreadViewHistory(key);assert.deepEqual(threadViewHistory(key),{before:1,hasMore:false});
submitThreadView(key,'fixture-client','fixture pending');
applyThreadViewChanges(key,{view_seq:11,rows:[{...row(4,11),client_message_id:'fixture-client'}]});
assert.equal(threadViewSlots(key)!.filter(s=>s.user.client_message_id==='fixture-client').length,1);
for(let i=5;i<1000;i++)applyThreadViewChanges(key,{view_seq:i+20,rows:[row(i,i+20)]});
assert.equal(threadViewSlots(key)!.length,100);
clearThreadView(key);generation=beginThreadView(key);commitThreadView(key,page([]),generation);assert.equal(threadViewPhase(key),'verified_empty');
console.log('thread view: generation, retained content, readiness, historical cursor, optimistic binding, unchanged identity, bounds and verified empty pass');

clearThreadView(key);generation=beginThreadView(key);commitThreadView(key,page([row(1)]),generation);
applyThreadViewChanges(key,{view_seq:2,rows:[{...row(1,2),status:'running'}]});
applyThreadViewTail(key,{item_id:'fixture-answer-1',field:'content',offset:14,text:'😀 fixture tail'});
assert.equal(threadViewSlots(key)![0].assistant.content,'fixture answer😀 fixture tail');
const beforeInvalid=threadViewSlots(key)![0];assert.equal(commitThreadView(key,{...page([]),chat:null as any},generation),false);assert.equal(threadViewSlots(key)![0],beforeInvalid);
assert.equal(applyThreadViewChanges(key,{epoch:2,view_seq:3,rows:[]}),false);
for(let i=2;i<=100;i++)applyThreadViewChanges(key,{view_seq:i+3,rows:[row(i,i+3)]});
commitThreadView(key,page(Array.from({length:20},(_,i)=>row(-i,500))),generation,true);
assert.equal(threadViewAtLatest(key),false);assert.ok(threadViewSlots(key)!.some(s=>s.rootTurnId==='fixture-turn--19'));
const historicalIds=threadViewSlots(key)!.map(s=>s.rootTurnId);
applyThreadViewChanges(key,{view_seq:1000,rows:[{...row(101,1000),status:'running'}]});
assert.deepEqual(threadViewSlots(key)!.map(s=>s.rootTurnId),historicalIds);assert.equal(threadViewStatus(key),'running');
console.log('thread view: invalid candidates, epoch reset, UTF16 tail, bounded historical browsing and independent activity pass');

// Walking beyond the retained window must be reversible: this simulates an
// old page evicting its tail, then the user scrolling back down through `after`.
clearThreadView(key);generation=beginThreadView(key);
const range=(from:number,to:number)=>Array.from({length:to-from+1},(_,index)=>row(from+index,1000+index));
commitThreadView(key,{...page(range(101,200),2000),next_before:101,has_more:true},generation);
commitThreadView(key,{...page(range(81,100),2001),next_before:81,has_more:true},generation,true);
assert.equal(threadViewSlots(key)!.length,100);assert.equal(threadViewSlots(key)![0].user.user_round,81);
assert.equal(threadViewWindow(key).hasNewer,true);
commitThreadView(key,{...page(range(181,200),2001),next_before:181,next_after:200,has_more:false,has_newer:false},generation,false,false,true);
assert.equal(threadViewSlots(key)!.length,100);assert.equal(threadViewSlots(key)![0].user.user_round,101);
assert.equal(threadViewSlots(key)!.at(-1)!.user.user_round,200);assert.equal(threadViewWindow(key).hasNewer,false);
assert.equal(threadViewWindow(key).before,101);
commitThreadView(key,{...page(range(81,100),2002),next_before:81,has_more:true},generation,true);
assert.equal(threadViewSlots(key)![0].user.user_round,81);
assert.equal(threadViewSlots(key)!.at(-1)!.user.user_round,180);
console.log('thread view: bidirectional bounded window preserves reversible history traversal pass');

clearThreadView(key);generation=beginThreadView(key);
const live={...row(1,10),status:'running',assistant:{item_id:'fixture-answer-1',content:'😀',durable_seq:5,content_offset:2,body:{body_id:'fixture-answer-1|stream',revision:0,bytes:4}}};
commitThreadView(key,page([live],10),generation);
const sendTail=(offset:number,text:string,base_seq=5)=>applyThreadViewTail(key,{item_id:'fixture-answer-1',content_offset:offset,text,base_seq});
sendTail(2,'fixture',6);assert.equal(threadViewSlots(key)![0].assistant.content,'😀');
sendTail(2,'fixture');sendTail(2,'fixture');sendTail(100,'hole');
assert.equal(threadViewSlots(key)![0].assistant.content,'😀fixture');
let offset=9;for(let n=0;n<300;n++){sendTail(offset,'😀tail');offset+=6;}
const long=threadViewSlots(key)![0].assistant;assert.ok(long.__thread_preview);assert.ok(long.__thread_tail);
assert.ok(new TextEncoder().encode(long.content).length<=2048);assert.ok(new TextEncoder().encode(long.__thread_tail).length<=2048);
// Another feeder can advance the head ahead of a row delivery. Its row revision
// still needs to land, while the older envelope cannot regress runtime status.
applyThreadViewChanges(key,{view_seq:100,runtime_status:'completed',rows:[]});
applyThreadViewChanges(key,{view_seq:20,runtime_status:'running',rows:[{...row(1,20),status:'completed',assistant:{item_id:'fixture-answer-1',content:'fixture sealed'}}]});
assert.equal(threadViewSlots(key)![0].assistant.content,'fixture sealed');assert.equal(threadViewStatus(key),'completed');assert.equal(threadViewCursor(key),100);
console.log('thread view: durable tail baseline, duplicates, holes, UTF16, finite live tail and interleaved feeder delivery pass');

clearThreadView(key);generation=beginThreadView(key);
assert.equal(commitThreadView(key,{...page([row(15,300)],300),epoch:-1,projection_state:'source'},generation),true);
assert.equal(threadViewPhase(key),'ready');assert.equal(threadViewCursor(key),-301);
assert.equal(threadViewSlots(key)![0].user.user_round,15);
assert.equal(commitThreadView(key,page([row(15,20)],20),generation),true);
assert.equal(threadViewCursor(key),20);assert.equal(threadViewSlots(key)![0].user.user_round,15);
console.log('thread view: bounded source window and projection epoch transition pass');


// Each model round has an independent assistant item.  A new reasoning stream
// must replace the row-level preview identity so the process renderer can append
// it after prior durable reasoning instead of reusing the completed entry.
clearThreadView(key);generation=beginThreadView(key);
const firstReasoning={...row(1,1),status:'running',reasoning_count:1,reasoning_item_id:'fixture-answer-1:text-1',assistant:{item_id:'fixture-answer-1:text-1',content:'',reasoning:'first thought',reasoning_offset:13}};
commitThreadView(key,page([firstReasoning],1),generation);
applyThreadViewTail(key,{item_id:'fixture-answer-1:text-1',field:'reasoning',offset:13,text:' one'});
const secondReasoning={...row(1,2),status:'running',reasoning_count:2,reasoning_item_id:'fixture-answer-1:text-2',assistant:{item_id:'fixture-answer-1:text-2',content:'',reasoning:'second thought',reasoning_offset:14}};
applyThreadViewChanges(key,{view_seq:2,rows:[secondReasoning]});
const activeReasoning=threadViewSlots(key)![0].assistant;
assert.equal(activeReasoning.reasoning_item_id,'fixture-answer-1:text-2');
assert.equal(activeReasoning.reasoning,'second thought');
assert.equal(activeReasoning.process_count,2);
console.log('thread view: active reasoning identity follows the newest model item pass');

clearThreadView(key);generation=beginThreadView(key);
commitThreadView(key,page([{...row(1,1),status:'queued',client_message_id:'fixture-queued'}]),generation);
removeThreadViewTurnByClientMessageId(key,'fixture-queued');
assert.equal(threadViewSlots(key)!.length,0);
commitThreadView(key,page([{...row(1,2),status:'running',client_message_id:'fixture-started'}],2),generation);
removeThreadViewTurnByClientMessageId(key,'fixture-started');
assert.equal(threadViewSlots(key)!.length,1);
console.log('thread view: only queued durable rows are removable by client message id pass');

clearThreadView(key);generation=beginThreadView(key);
commitThreadView(key,page([{...row(1,1),status:'running',process_activity:{kind:'thinking'},assistant:{item_id:'fixture-answer-1',content:'',reasoning:'fixture thought',reasoning_offset:15}}]),generation);
applyThreadViewTail(key,{item_id:'fixture-answer-1',field:'reasoning',offset:15,text:' latest'});
assert.equal(threadViewSlots(key)![0].assistant.process_activity.kind,'thinking');
applyThreadViewTail(key,{item_id:'fixture-answer-1',field:'content',content_offset:0,text:'fixture reply',base_seq:0});
assert.equal(threadViewSlots(key)![0].assistant.process_activity.kind,'responding');
applyThreadViewChanges(key,{view_seq:2,rows:[{...row(1,2),status:'running',process_activity:{kind:'tool',tool:'fixture_tool'}}]});
assert.equal(threadViewSlots(key)![0].assistant.process_activity.tool,'fixture_tool');
console.log('thread view: current process phase follows reasoning, content and tool changes pass');

clearThreadView(key);generation=beginThreadView(key);
commitThreadView(key,page([{...row(1,1),status:'running',process_activity:{kind:'tool',tool:'fixture_tool',item_id:'fixture-call',created_seq:20},assistant:{item_id:'fixture-answer-1',created_seq:10,content:'',reasoning:'fixture thought',reasoning_offset:15}}]),generation);
applyThreadViewTail(key,{item_id:'fixture-answer-1',field:'reasoning',offset:15,text:' late'});
assert.equal(threadViewSlots(key)![0].assistant.reasoning,'fixture thought late');
assert.equal(threadViewSlots(key)![0].assistant.process_activity.kind,'tool');
applyThreadViewTail(key,{item_id:'fixture-answer-1',field:'content',content_offset:0,text:'fixture late reply',base_seq:0});
assert.equal(threadViewSlots(key)![0].assistant.process_activity.kind,'tool');
console.log('thread view: late model tails cannot replace a newer tool phase pass');

// Display-only delivery must consume the composer queue and reject stale HTTP rows.
import { chatQueueState, applyItems } from '../../src/stores/chatQueueLiveState';
import { projectQueueItems, isParkedQueueTurn } from '../../src/stores/chatQueueProjection';
clearThreadView(key); generation = beginThreadView(key);
commitThreadView(key, { ...page([ { ...row(1, 1), status: 'running' } ]), runtime_status: 'running' }, generation);
submitThreadView(key, 'fixture-queued-client', 'queued input');
assert.equal(threadViewStatus(key), 'running');
const pendingQueue = threadViewSlots(key)!.at(-1)!;
assert.equal(isParkedQueueTurn([], pendingQueue.user, pendingQueue.assistant), true);
const parked = projectQueueItems([{queue_id:'fixture-queue', root_turn_id:'fixture-turn-2', client_message_id:'fixture-queued-client'}]);
chatQueueState.sessionId = key; applyItems(key, parked);
applyThreadViewChanges(key, { view_seq: 12, runtime_status: 'running', rows: [{ ...row(2, 12), client_message_id:'fixture-queued-client', status: 'running' }] });
assert.equal(chatQueueState.items.length, 0);
applyItems(key, parked); assert.equal(chatQueueState.items.length, 0);
// An idle submission remains mounted while the server accepts it before execution.
clearThreadView(key); generation = beginThreadView(key); commitThreadView(key, page([]), generation);
submitThreadView(key, 'fixture-admission', 'input');
applyThreadViewChanges(key, { view_seq: 20, rows: [{ ...row(3,20), client_message_id:'fixture-admission', status:'accepted', assistant:{item_id:'',content:''} }] });
const admitted = threadViewSlots(key)![0];
assert.equal(isParkedQueueTurn([], admitted.user, admitted.assistant), false);
// A durable thinking preview may lag behind the live answer tail for the same item.
applyThreadViewChanges(key, {view_seq:21, rows:[{...row(3,21), status:'running', assistant:{item_id:'fixture-phase',content:'',created_seq:10}, process_activity:{kind:'thinking',item_id:'fixture-phase',created_seq:10}}]});
applyThreadViewTail(key, {item_id:'fixture-phase',field:'content',offset:0,text:'answer'});
applyThreadViewChanges(key, {view_seq:22, rows:[{...row(3,22),status:'running',assistant:{item_id:'fixture-phase',content:'',created_seq:10},process_activity:{kind:'thinking',item_id:'fixture-phase',created_seq:10}}]});
applyThreadViewTail(key, {item_id:'fixture-phase',field:'reasoning',offset:0,text:'late thought'});
assert.equal(threadViewSlots(key)![0].assistant.process_activity.kind,'responding');
assert.equal(threadViewSlots(key)![0].assistant.reasoning,'late thought');
console.log('thread view: queue handoff, stale response, admission continuity and monotonic phase pass');

// Historical pages and interleaved feeder rows cannot overwrite the head.
import {threadViewActiveTurnId,threadViewOwnsExecution} from '../../src/realtime/chat/threadViewStore';
clearThreadView(key); generation=beginThreadView(key);
commitThreadView(key,{...page([row(1,50)],50),runtime_status:'running',active_turn_id:'fixture-execution'},generation);
applyThreadViewChanges(key,{view_seq:49,runtime_status:'queued',active_turn_id:'fixture-stale',rows:[{...row(2,49),status:'queued'}]});
assert.equal(threadViewStatus(key),'running');
assert.equal(threadViewActiveTurnId(key),'fixture-execution');
assert.equal(threadViewOwnsExecution(key),true);
commitThreadView(key,{...page([row(0,60)],60),runtime_status:'completed',active_turn_id:''},generation,true);
commitThreadView(key,{...page([row(2,61)],61),runtime_status:'queued',active_turn_id:'fixture-page'},generation,false,false,true);
assert.equal(threadViewStatus(key),'running');
assert.equal(threadViewActiveTurnId(key),'fixture-execution');
applyThreadViewChanges(key,{view_seq:62,runtime_status:'completed',active_turn_id:'',rows:[]});
applyThreadViewChanges(key,{view_seq:61,rows:[{...row(3,61),status:'running'}]});
assert.equal(threadViewStatus(key),'completed');
assert.equal(threadViewActiveTurnId(key),'');
// A background thread owns its queue independently; no HTTP refresh required.
const background='fixture-background';
applyThreadViewChanges(background,{epoch:1,view_seq:1,runtime_status:'running',rows:[{
  ...row(1),root_turn_id:'fixture-background-turn',client_message_id:'fixture-background-client',status:'queued',
  queued_input:{queue_id:'fixture-background-queue',reason:'input',priority:0}
}]});
assert.equal(chatQueueState.sessionId,key);
chatQueueState.sessionId=background;
assert.equal(chatQueueState.items[0].queueId,'fixture-background-queue');
applyThreadViewChanges(background,{epoch:1,view_seq:2,runtime_status:'running',rows:[{
  ...row(1,2),root_turn_id:'fixture-background-turn',client_message_id:'fixture-background-client',status:'running'
}]});
assert.equal(chatQueueState.items.length,0);
applyItems(background,projectQueueItems([{queue_id:'fixture-background-queue',client_message_id:'fixture-background-client'}]));
assert.equal(chatQueueState.items.length,0);
console.log('thread view: independent monotonic head, old pages, background queue isolation and stale snapshots pass');

// Accepted input must not flash into the transcript before its queue decision.
clearThreadView(key);generation=beginThreadView(key);
commitThreadView(key,{...page([row(1,1)]),runtime_status:'running',active_turn_id:'fixture-active'},generation);
submitThreadView(key,'fixture-busy-admission','fixture input');
applyThreadViewChanges(key,{epoch:1,view_seq:2,runtime_status:'running',active_turn_id:'fixture-active',rows:[{
  ...row(2,2),client_message_id:'fixture-busy-admission',status:'accepted',assistant:{item_id:'',content:''}
}]});
const busyAdmission=threadViewSlots(key)!.at(-1)!;
assert.equal(isParkedQueueTurn([],busyAdmission.user,busyAdmission.assistant),true);
console.log('thread view: busy admission preserves composer ownership until dispatch pass');

import {applyThreadViewHead,settlePendingThreadView,threadViewHeadVersion} from '../../src/realtime/chat/threadViewStore';
const replayCursor=threadViewCursor(key);
assert.equal(applyThreadViewHead(key,{epoch:1,view_seq:200,runtime_status:'completed',active_turn_id:''}),true);
assert.equal(threadViewStatus(key),'completed');
// A fresh submission bridges the previous terminal head until admission;
// replaying that same head must not report completion for the new request.
submitThreadView(key,'fixture-terminal-pending','next input');
assert.equal(threadViewStatus(key),'running');
assert.equal(threadViewActiveTurnId(key),'pending:fixture-terminal-pending');
assert.equal(threadViewCursor(key),replayCursor);
assert.equal(applyThreadViewHead(key,{epoch:1,view_seq:199,runtime_status:'running',active_turn_id:'fixture-stale'}),false);
applyThreadViewChanges(key,{epoch:1,view_seq:200,runtime_status:'completed',active_turn_id:'',rows:[]});
assert.equal(threadViewStatus(key),'running');
assert.equal(threadViewHeadVersion(key),'1:200');
// A newer authoritative completion wins even if replay has not yet delivered
// the row that binds this client id. No permanent stop-button state.
assert.equal(applyThreadViewHead(key,{epoch:1,view_seq:201,runtime_status:'completed',active_turn_id:''}),true);
assert.equal(threadViewStatus(key),'completed');
assert.equal(threadViewActiveTurnId(key),'');
console.log('thread view: detached catalog head advances without skipping transcript replay pass');

submitThreadView(key,'fixture-new-admission','input');
assert.equal(threadViewStatus(key),'running');
applyThreadViewChanges(key,{epoch:1,view_seq:202,runtime_status:'running',active_turn_id:'fixture-turn-4',rows:[{
  ...row(4,202),client_message_id:'fixture-new-admission',status:'accepted'
}]});
assert.equal(threadViewStatus(key),'running');
assert.equal(threadViewActiveTurnId(key),'fixture-turn-4');
submitThreadView(key,'fixture-parked','waiting input');
assert.equal(threadViewStatus(key),'running');
assert.equal(threadViewActiveTurnId(key),'fixture-turn-4');
applyThreadViewChanges(key,{epoch:1,view_seq:203,runtime_status:'completed',active_turn_id:'',rows:[row(4,203)]});
assert.equal(threadViewStatus(key),'completed');
for (const status of ['failed','cancelled','rejected']) {
  const client=`fixture-submit-${status}`;
  submitThreadView(key,client,'input');
  assert.equal(threadViewStatus(key),'running');
  settlePendingThreadView(key,client,status);
  assert.equal(threadViewStatus(key),'completed');
  assert.equal(threadViewActiveTurnId(key),'');
}
assert.equal(threadViewStatus(background),'running');
console.log('thread view: follow-up admission, binding, queued input and submission failure settle independently pass');

// The committed presentation window is the real page source: a plan must be
// available before lazy workflow hydration, and remain stable on reload.
import { resolveChatPlanDismissKey } from '../../src/realtime/chat/chatPlan';
import { summarizePlanProgress } from '../../src/components/chat/composerActivity';
clearThreadView(key); generation=beginThreadView(key);
const plan={item_id:'fixture-plan',revision:1,created_seq:2,explanation:'Fixture progress',
  steps:[{step:'First step',status:'in_progress' as const},{step:'Second step',status:'pending' as const}]};
const planned={...row(1,1),status:'running',assistant:{item_id:'',content:''},plan};
commitThreadView(key,page([planned]),generation);
const planMessage=()=>threadViewSlots(key)![0].assistant;
assert.deepEqual(summarizePlanProgress(planMessage().plan),{total:2,done:0});
assert.equal(threadViewSlots(key)![0].user.plan,undefined);
const dismissed=new Set([resolveChatPlanDismissKey(planMessage())]);
applyThreadViewChanges(key,{view_seq:2,rows:[{...planned,revision:2}]});
assert.equal(dismissed.has(resolveChatPlanDismissKey(planMessage())),true);
const nextPlan={...plan,revision:2,steps:[{step:'First step',status:'completed' as const},{step:'Second step',status:'in_progress' as const}]};
applyThreadViewChanges(key,{view_seq:3,rows:[{...planned,revision:3,plan:nextPlan}]});
assert.deepEqual(summarizePlanProgress(planMessage().plan),{total:2,done:1});
assert.equal(dismissed.has(resolveChatPlanDismissKey(planMessage())),false);
const completed={...row(1,4),plan:nextPlan};
applyThreadViewChanges(key,{view_seq:4,rows:[completed]});
clearThreadView(key);generation=beginThreadView(key);commitThreadView(key,page([completed],4),generation);
assert.deepEqual(planMessage().plan,nextPlan);
console.log('thread view: durable composer plan, progress, dismiss identity and reload pass');
