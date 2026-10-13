import assert from 'node:assert/strict';
import {beginThreadView,commitThreadView,threadViewSlots,threadViewPhase,threadViewCursor,applyThreadViewChanges,submitThreadView,failThreadView,clearThreadView,applyThreadViewTail,threadViewAtLatest,threadViewStatus} from '../../src/realtime/chat/threadViewStore';
const row=(round:number,revision=1)=>({root_turn_id:`fixture-turn-${round}`,user_round:round,revision,status:'completed',user:{item_id:`fixture-user-${round}`,content:'fixture input'},assistant:{item_id:`fixture-answer-${round}`,content:'fixture answer'},workflow_count:0,failed_count:0});
const page=(chat:any[],seq=1)=>({session_id:'fixture-thread',epoch:1,view_seq:seq,projection_state:'ready',thread_empty:chat.length===0,latest_round:chat.length,chat,trajectory:[],tiles:[],tile_level:0,next_before:1,has_more:false});
const key='fixture-thread';
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
