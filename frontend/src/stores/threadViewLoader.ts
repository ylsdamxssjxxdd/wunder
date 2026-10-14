import { getThreadViewBootstrap, getThreadViewChatPage } from '@/api/chat';
import { beginThreadView, commitThreadView, failThreadView, threadViewSlots, threadViewHistory, threadViewWindow, threadViewGeneration, threadViewHasNewerPage, threadViewStatus } from '@/realtime/chat/threadViewStore';
import { clearSessionWatcher, setSessionLoading } from './chatRuntimeControls';
import { getHistoryState, cacheSessionMessages, ensureRuntime } from './chatRuntimeState';
import { persistActiveSession, patchSessionRuntimeFields, syncGoalFromSessionRecord } from './chatPersist';
import { syncChatThreadShell } from './chatThreadEffects';
import { startSessionWatcher } from './chatWatcher';
import { chatPerf } from '@/utils/chatPerf';

interface OpenTask {
  store: any;
  controller: AbortController;
  startWatcher: boolean;
  promise: Promise<any>;
}
const inFlightOpens = new Map<string, OpenTask>();
const prefetches=new Map<string,AbortController>();
const prefetched=new Map<string,{data:any,time:number}>();
const pageRequests = new Map<string, AbortController>();
const pageRetryAfter = new Map<string, number>();
export function resetThreadViewLoads() {
  for (const request of pageRequests.values()) request.abort();
  pageRequests.clear();
  pageRetryAfter.clear();
  for (const task of inFlightOpens.values()) task.controller.abort();
  inFlightOpens.clear();
  for (const controller of prefetches.values()) controller.abort();
  prefetches.clear();
  prefetched.clear();
}
export async function prefetchThreadView(key:string) {
  if(prefetches.has(key)||prefetches.size>=2)return null;
  const controller=new AbortController();prefetches.set(key,controller);
  try{const r=await getThreadViewBootstrap(key,{kind:'chat'},{signal:controller.signal});if(controller.signal.aborted)return null;
    prefetched.set(key,{data:r.data.data,time:Date.now()});while(prefetched.size>8)prefetched.delete(prefetched.keys().next().value!);return r.data.data.session;
  }finally{if(prefetches.get(key)===controller)prefetches.delete(key);}
}
async function openThreadViewInternal(store: any, key: string, task: OpenTask) {
  const started=performance.now();
  for(const c of prefetches.values())c.abort();prefetches.clear();
  const same=store.activeSessionId===key;
  if(!same) clearSessionWatcher();
  const controller=task.controller;
  const generation=beginThreadView(key);store.activeSessionId=key;
  const history=getHistoryState(key);history.loading=true;
  store.messages=(threadViewSlots(key)??[]).flatMap(s=>[s.user,s.assistant]);
  // Once the bounded view has been committed, the user already has a valid
  // conversation on screen. Runtime/watcher hydration is best effort and must
  // not turn a successful open into a misleading "open failed" toast.
  let committedSession: any = null;
  let viewCommitted = false;
  try {
    const cached=prefetched.get(key);prefetched.delete(key);
    const response=cached&&Date.now()-cached.time<1500?{data:{data:cached.data}}:await getThreadViewBootstrap(key,{kind:'chat'},{signal:controller.signal});
    if(controller.signal.aborted || store.activeSessionId!==key || threadViewGeneration(key)!==generation)return null;
    const data=response.data.data;
    // Recovery may have committed a newer page while this request was in
    // flight. Keep that authoritative page and finish opening successfully.
    // Invalid payloads, different epochs and obsolete generations still fail.
    if(!commitThreadView(key,data.page,generation) && !threadViewHasNewerPage(key,data.page,generation)) {
      throw new Error('线程展示响应无效，请重试');
    }
    viewCommitted = true;
    const session=patchSessionRuntimeFields(data.session);
    committedSession = session;
    const index=store.sessions.findIndex((s:any)=>s.id===key);
    if(index>=0)store.sessions[index]={...store.sessions[index],...session};else store.sessions.unshift(session);
    syncGoalFromSessionRecord(store,session);
    store.messages=(threadViewSlots(key)??[]).flatMap(s=>[s.user,s.assistant]);cacheSessionMessages(key,store.messages);
    const cursor=threadViewHistory(key);
    history.beforeId=cursor.before;history.hasMore=cursor.hasMore;
    store.draftAgentId=String(session.agent_id??'');persistActiveSession(key,store.draftAgentId);
    ensureRuntime(key);syncChatThreadShell(store,key);setSessionLoading(store,key,['running','queued','waiting_input','waiting_approval'].includes(threadViewStatus(key) ?? 'idle'));
    if(task.startWatcher)startSessionWatcher(store,key);
    requestAnimationFrame(()=>requestAnimationFrame(()=>{if(store.activeSessionId===key&&threadViewGeneration(key)===generation)chatPerf.recordDuration('chat_view_content_paint',performance.now()-started,{rows:data.page.chat.length});}));
    return session;
  } catch(error) {
    if(controller.signal.aborted || store.activeSessionId!==key || threadViewGeneration(key)!==generation)return null;
    failThreadView(key,generation,error);
    if (viewCommitted) {
      // Keep the committed transcript usable while retaining a diagnostic for
      // the runtime bootstrap problem. A later realtime refresh can repair it.
      console.error('[thread-view] post-commit runtime hydration failed', error);
      return committedSession;
    }
    throw error;
  } finally {if(inFlightOpens.get(key)===task){inFlightOpens.delete(key);history.loading=false;}}
}

export async function openThreadView(store: any, key: string, options: any = {}) {
  const existing = inFlightOpens.get(key);
  if (existing && existing.store === store && !existing.controller.signal.aborted && store.activeSessionId === key) {
    existing.startWatcher ||= options.startWatcherAfterHydration !== false;
    return existing.promise;
  }
  // Remove cancelled entries synchronously: A -> B -> A must start a fresh A
  // request even before the cancelled promise's finally callback has run.
  for (const [id, pending] of inFlightOpens) {
    pending.controller.abort();
    getHistoryState(id).loading = false;
    inFlightOpens.delete(id);
  }
  const task: OpenTask = { store, controller: new AbortController(), startWatcher: options.startWatcherAfterHydration !== false, promise: Promise.resolve(null) };
  inFlightOpens.set(key, task);
  task.promise = openThreadViewInternal(store, key, task);
  return task.promise;
}

export const olderThreadView = (store: any, key: string) => pageThreadView(store, key, false);
export const newerThreadView = (store: any, key: string) => pageThreadView(store, key, true);

async function pageThreadView(store: any, key: string, forward: boolean) {
  const history = getHistoryState(key);
  const cursor = threadViewWindow(key);
  const edge = forward ? cursor.after : cursor.before;
  if (store.activeSessionId !== key || history.loading || pageRequests.has(key) ||
      Date.now() < (pageRetryAfter.get(key) ?? 0) ||
      !(forward ? cursor.hasNewer : cursor.hasMore) || edge === null) return [];
  history.loading = true;
  const controller = new AbortController();
  pageRequests.set(key, controller);
  const generation = threadViewGeneration(key);
  try {
    const response = await getThreadViewChatPage(key,
      { [forward ? 'after' : 'before']: edge, limit: 20 }, { signal: controller.signal });
    if (controller.signal.aborted || store.activeSessionId !== key || threadViewGeneration(key) !== generation) return [];
    const page = response.data.data.page;
    if (page.degraded || page.projection_state === 'degraded') throw new Error('History temporarily unavailable');
    if (!commitThreadView(key, page, generation, !forward, false, forward)) {
      await openThreadView(store, key);
      return [];
    }
    pageRetryAfter.delete(key);
    const next = threadViewWindow(key);
    history.beforeId = next.before;
    history.hasMore = next.hasMore;
    const messages = (threadViewSlots(key) ?? []).flatMap(s => [s.user, s.assistant]);
    cacheSessionMessages(key, messages);
    store.messages = messages;
    return page.chat.length ? messages : [];
  } catch (error) {
    if (!controller.signal.aborted && threadViewGeneration(key) === generation) {
      // A temporary read failure must not erase the pagination cursor. The next
      // scroll retries after a cooldown; no timer polls an idle history view.
      pageRetryAfter.set(key, Date.now() + 2000);
      while (pageRetryAfter.size > 32) pageRetryAfter.delete(pageRetryAfter.keys().next().value!);
      console.warn('[thread-view] history page unavailable', error);
    }
    return [];
  } finally {
    if (pageRequests.get(key) === controller) pageRequests.delete(key);
    if (threadViewGeneration(key) === generation) history.loading = false;
  }
}
