import { getThreadViewBootstrap, getThreadViewChatPage } from '@/api/chat';
import { beginThreadView, commitThreadView, failThreadView, threadViewSlots, threadViewHistory, threadViewGeneration } from '@/realtime/chat/threadViewStore';
import { clearSessionWatcher, setSessionLoading } from './chatRuntimeControls';
import { getHistoryState, cacheSessionMessages, ensureRuntime } from './chatRuntimeState';
import { persistActiveSession, patchSessionRuntimeFields, syncGoalFromSessionRecord } from './chatPersist';
import { syncChatThreadShell } from './chatThreadEffects';
import { startSessionWatcher } from './chatWatcher';
import { chatPerf } from '@/utils/chatPerf';

const loads = new Map<string, AbortController>();
const prefetches=new Map<string,AbortController>();
const prefetched=new Map<string,{data:any,time:number}>();
export async function prefetchThreadView(key:string) {
  if(prefetches.has(key)||prefetches.size>=2)return null;
  const controller=new AbortController();prefetches.set(key,controller);
  try{const r=await getThreadViewBootstrap(key,{kind:'chat'},{signal:controller.signal});if(controller.signal.aborted)return null;
    prefetched.set(key,{data:r.data.data,time:Date.now()});while(prefetched.size>8)prefetched.delete(prefetched.keys().next().value!);return r.data.data.session;
  }finally{if(prefetches.get(key)===controller)prefetches.delete(key);}
}
export async function openThreadView(store: any, key: string, options: any = {}) {
  const started=performance.now();
  for(const c of prefetches.values())c.abort();prefetches.clear();
  const same=store.activeSessionId===key;
  if(!same) {clearSessionWatcher();for(const [id,c] of loads)if(id!==key)c.abort();}
  loads.get(key)?.abort();const controller=new AbortController();loads.set(key,controller);
  const generation=beginThreadView(key);store.activeSessionId=key;
  const history=getHistoryState(key);history.loading=true;
  store.messages=(threadViewSlots(key)??[]).flatMap(s=>[s.user,s.assistant]);
  try {
    const cached=prefetched.get(key);prefetched.delete(key);
    const response=cached&&Date.now()-cached.time<1500?{data:{data:cached.data}}:await getThreadViewBootstrap(key,{kind:'chat'},{signal:controller.signal});
    if(controller.signal.aborted || store.activeSessionId!==key)return null;
    const data=response.data.data;
    const session=patchSessionRuntimeFields(data.session);
    const index=store.sessions.findIndex((s:any)=>s.id===key);
    if(index>=0)store.sessions[index]={...store.sessions[index],...session};else store.sessions.unshift(session);
    syncGoalFromSessionRecord(store,session);
    if(!commitThreadView(key,data.page,generation)){if(data.page.projection_state!=='ready'){startSessionWatcher(store,key);return null;}throw new Error('展示响应已过期，请重试');}
    store.messages=(threadViewSlots(key)??[]).flatMap(s=>[s.user,s.assistant]);cacheSessionMessages(key,store.messages);
    history.beforeId=data.page.next_before;history.hasMore=data.page.has_more;
    store.draftAgentId=String(session.agent_id??'');persistActiveSession(key,store.draftAgentId);
    ensureRuntime(key);syncChatThreadShell(store,key);setSessionLoading(store,key,data.page.chat.some((r:any)=>['running','queued','waiting_input','waiting_approval'].includes(r.status)));
    if(options.startWatcherAfterHydration!==false)startSessionWatcher(store,key);
    requestAnimationFrame(()=>requestAnimationFrame(()=>{if(store.activeSessionId===key&&threadViewGeneration(key)===generation)chatPerf.recordDuration('chat_view_content_paint',performance.now()-started,{rows:data.page.chat.length});}));
    return session;
  } catch(error) {
    if(controller.signal.aborted)return null;
    failThreadView(key,generation,error);
    throw error;
  } finally {if(loads.get(key)===controller){loads.delete(key);history.loading=false;}}
}
export async function olderThreadView(store:any,key:string) {
  const history=getHistoryState(key);const cursor=threadViewHistory(key);
  if(history.loading||!cursor.hasMore||cursor.before===null)return [];
  history.loading=true;
  const generation=threadViewGeneration(key);
  try {
    const response=await getThreadViewChatPage(key,{before:cursor.before,limit:20});
    const page=response.data.data.page;if(!commitThreadView(key,page,generation,true))return [];
    history.beforeId=page.next_before;history.hasMore=page.has_more;
    const messages=(threadViewSlots(key)??[]).flatMap(s=>[s.user,s.assistant]);
    cacheSessionMessages(key,messages);if(store.activeSessionId===key)store.messages=messages;
    return messages;
  } finally {history.loading=false;}
}
