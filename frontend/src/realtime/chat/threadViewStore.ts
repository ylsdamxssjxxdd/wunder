// AI生成
// The display window is independent of connection and execution-log state.
import { shallowRef, markRaw } from 'vue';

export interface ThreadBodyRef { body_id: string; revision: number; bytes: number }
export interface ViewMessage { item_id: string; content: string; reasoning?: string; durable_seq?: number; content_offset?: number; reasoning_offset?: number; tail_offset?: number; body?: ThreadBodyRef; feedback?: unknown; stats?: unknown; tail?: string; failure?: string; attachments?: unknown[] }
export interface ChatViewRow { root_turn_id: string; execution_turn_id?:string; user_round: number; client_message_id?: string; revision: number; status: string; user: ViewMessage; assistant: ViewMessage; workflow_count: number; reasoning_count?: number; reasoning_item_id?: string; failed_count: number; subagents?: any[] }
export interface ThreadViewPage { runtime_status?:string;active_turn_id?:string; session_id: string; epoch: number; view_seq: number; projection_state: string; degraded?: boolean; diagnostic?: string | null; thread_empty: boolean | null; latest_round: number; chat: ChatViewRow[]; trajectory: any[]; tiles: any[]; tile_level: number; next_before: number | null; has_more: boolean; next_after?: number | null; has_newer?: boolean }
export type ViewPhase = 'opening' | 'ready' | 'verified_empty' | 'refreshing' | 'error';
export interface ViewSlot { key: string; rootTurnId: string; user: Record<string, any>; assistant: Record<string, any> }
interface Entry { phase: ViewPhase; epoch: number; seq: number; generation: number; rows: Map<string, ChatViewRow>; slots: Map<string, ViewSlot>; order: string[]; next: number | null; more: boolean; bytes: number; error: string; atLatest: boolean; latestRound: number; latestStatus: string }
const entries = new Map<string, Entry>();
export const threadViewClock = shallowRef(0);
const MAX_BYTES = 48 * 1024 * 1024;
const MAX_ROWS = 100;
let activeKey = '';
let nextGeneration = 0;
function entry(key: string): Entry {
  let e = entries.get(key);
  if (!e) { e = { phase: 'opening', epoch: 0, seq: 0, generation: 0, rows: new Map(), slots: new Map(), order: [], next: null, more: false, bytes: 0, error: '', atLatest: true, latestRound: 0, latestStatus: 'idle' }; entries.set(key, e); }
  return e;
}
let tailTimer:ReturnType<typeof setTimeout>|null=null;
function notify() { threadViewClock.value++; }
function notifyTail(){if(tailTimer===null)tailTimer=setTimeout(()=>{tailTimer=null;notify();},16);}
export function beginThreadView(key: string): number { activeKey = key; const e = entry(key); e.phase = e.rows.size ? 'refreshing' : 'opening'; e.error = ''; e.generation = ++nextGeneration; notify(); return e.generation; }
export function threadViewGeneration(key: string): number { return entry(key).generation; }
export function threadViewPhase(key: string): ViewPhase | undefined { void threadViewClock.value; return entries.get(key)?.phase; }
export function threadViewCursor(key: string): number { const e=entries.get(key);return e?.epoch===-1 ? -((e.seq??0)+1) : e?.seq??0; }
export function threadViewHistory(key: string) { const e = entry(key); return { before: e.next, hasMore: e.more }; }
/**
 * Cursor pair for a bounded transcript window. Keep the legacy history shape
 * stable for callers that only walk toward the beginning of a thread.
 */
export function threadViewWindow(key: string) {
  const e = entry(key);
  return {
    before: e.next,
    hasMore: e.more,
    after: e.rows.get(e.order.at(-1) ?? '')?.user_round ?? null,
    hasNewer: !e.atLatest,
  };
}
export function pauseThreadViewHistory(key: string) { const e = entry(key); e.more = false; notify(); }
/** Row status is server vocabulary; message bubbles and audit hooks speak the UI one. */
export const uiMessageStatus = (status: string) =>
  status === 'completed' ? 'final' : status === 'rejected' ? 'failed' : status;
function materialize(row: ChatViewRow): ViewSlot {
  const running = ['running','queued','waiting_input','waiting_approval'].includes(row.status);
  const make = (role: string, m: ViewMessage) => markRaw({ id: `tturn:${row.root_turn_id}:${role}`, role, content: m.content, item_id: m.item_id,
    turn_id: row.execution_turn_id??row.root_turn_id, root_turn_id: row.root_turn_id, user_round: row.user_round, client_message_id: row.client_message_id,
    __runtime_projected: true, __thread_view:true, final: !running, state:role==='assistant'?(running?'streaming':row.status==='completed'?'final':row.status):'final', __thread_body: m.body, __thread_preview: Boolean(m.body && m.body.bytes > new TextEncoder().encode(m.content).length),
    streaming: role === 'assistant' && running, status: role === 'assistant' ? row.status : 'completed',
    // The process badge counts every visible process entry.  The server keeps
    // tool counts separately, so include the aggregated reasoning entry here.
    workflow_count: row.workflow_count, reasoning_count: Number(row.reasoning_count || 0), reasoning_item_id: row.reasoning_item_id || '', process_count: row.workflow_count + Number(row.reasoning_count || 0) + (role === 'assistant' && String(m.reasoning || '').trim() && !Number(row.reasoning_count || 0) ? 1 : 0), workflow_failed_count: row.failed_count, feedback: m.feedback, stats: m.stats, reasoning: m.reasoning || '',
    __thread_tail: m.tail, failureDetail:m.failure, attachments:m.attachments,
    __thread_tail_offset: m.tail_offset,
    workflowItems: [], ...(role==='assistant'&&Array.isArray(row.subagents)&&row.subagents.length>0?{subagents:row.subagents}:{}), failed: role === 'assistant' && ['failed','rejected'].includes(row.status), stopped: row.status === 'cancelled' });
  return markRaw({ key: row.client_message_id ? `turn:client:${row.client_message_id}` : `turn:${row.root_turn_id}`, rootTurnId: row.root_turn_id, user: make('user', row.user), assistant: make('assistant', row.assistant) });
}
function utf16Units(text: string): number {
  return [...text].reduce((total, char) => total + char.length, 0);
}
function mergeTail(previous: ViewMessage, next: ViewMessage): ViewMessage {
  const oldText = String(previous.tail || '');
  const newText = String(next.tail || '');
  if (!oldText || !newText) return oldText && !newText ? { ...next, tail: oldText, tail_offset: previous.tail_offset } : next;
  const oldEnd = Number(previous.tail_offset || previous.content_offset || utf16Units(previous.content));
  const newEnd = Number(next.tail_offset || next.content_offset || utf16Units(next.content));
  if (!Number.isSafeInteger(oldEnd) || !Number.isSafeInteger(newEnd)) return next;
  const oldStart = oldEnd - utf16Units(oldText);
  const newStart = newEnd - utf16Units(newText);
  if (newStart >= oldEnd) return next;
  if (oldStart >= newEnd) return previous;
  const start = Math.min(oldStart, newStart);
  const end = Math.max(oldEnd, newEnd);
  const units = new Map<number, string>();
  let cursor = oldStart;
  for (const char of oldText) { units.set(cursor, char); cursor += char.length; }
  cursor = newStart;
  for (const char of newText) { units.set(cursor, char); cursor += char.length; }
  let merged = '';
  for (let at = start; at < end;) {
    const char = units.get(at);
    if (!char) break;
    merged += char;
    at += char.length;
  }
  return { ...next, tail: merged.length > 2048 ? merged.slice(-2048) : merged, tail_offset: end };
}
function merge(e: Entry, row: ChatViewRow) {
  const old = e.rows.get(row.root_turn_id);
  if (old && old.revision >= row.revision) return;
  if(old && old.assistant.item_id===row.assistant.item_id && ['running','queued','waiting_input','waiting_approval'].includes(row.status)) {
    const oldContentOffset = Number(old.assistant.content_offset || 0);
    const newContentOffset = Number(row.assistant.content_offset || 0);
    const oldReasoningOffset = Number(old.assistant.reasoning_offset || 0);
    const newReasoningOffset = Number(row.assistant.reasoning_offset || 0);
    const oldContentLength = String(old.assistant.content || '').length;
    const newContentLength = String(row.assistant.content || '').length;
    const oldReasoningLength = String(old.assistant.reasoning || '').length;
    const newReasoningLength = String(row.assistant.reasoning || '').length;
    const keepOldContent = oldContentOffset > newContentOffset ||
      (oldContentOffset === newContentOffset && oldContentLength > newContentLength);
    const keepOldReasoning = oldReasoningOffset > newReasoningOffset ||
      (oldReasoningOffset === newReasoningOffset && oldReasoningLength > newReasoningLength);
    const assistant = mergeTail(old.assistant, row.assistant);
    if (keepOldContent || keepOldReasoning || assistant.tail !== row.assistant.tail) {
      row={...row,assistant:{...row.assistant,
        ...(keepOldContent ? {content:old.assistant.content,tail:old.assistant.tail,tail_offset:old.assistant.tail_offset,content_offset:oldContentOffset} : {}),
        ...(keepOldReasoning ? {reasoning:old.assistant.reasoning,reasoning_offset:oldReasoningOffset} : {}),
        tail: assistant.tail, tail_offset: assistant.tail_offset,
        body:row.assistant.body?{...row.assistant.body,bytes:Math.max(row.assistant.body.bytes,old.assistant.body?.bytes??0)}:old.assistant.body}};
    }
  }
  e.rows.set(row.root_turn_id, markRaw(row)); e.slots.set(row.root_turn_id, materialize(row));
  if (!old) e.order.push(row.root_turn_id);
  if (row.client_message_id) {
    const pending = `pending:${row.client_message_id}`;
    if (pending !== row.root_turn_id) { e.rows.delete(pending); e.slots.delete(pending); e.order = e.order.filter(id => id !== pending); }
  }
}
function bound(e: Entry, historical = false) {
  e.order.sort((a,b) => (e.rows.get(a)?.user_round ?? 0) - (e.rows.get(b)?.user_round ?? 0));
  const evictsOlder = !historical && e.order.length > MAX_ROWS;
  while (e.order.length > MAX_ROWS) { const id=historical ? e.order.pop()! : e.order.shift()!;e.rows.delete(id);e.slots.delete(id); if(historical)e.atLatest=false; else e.more=true; }
  // The other edge was evicted during forward navigation or a live append.
  // Its next read must start at the retained boundary, not the old page cursor.
  if ((evictsOlder || e.next === null) && e.order.length) e.next = e.rows.get(e.order[0])!.user_round;
  e.bytes = e.order.reduce((n,id) => { const r=e.rows.get(id)!; return n+2*(r.user.content.length+r.assistant.content.length+(r.assistant.reasoning?.length??0)+(r.assistant.tail?.length??0))+4096; }, 0);
  let total = [...entries.values()].reduce((n,v)=>n+v.bytes,0);
  for (const [key,v] of entries) { if (total <= MAX_BYTES && entries.size <= 32) break; if (key === activeKey) continue; entries.delete(key);total-=v.bytes; }
}
function validRows(rows: unknown): rows is ChatViewRow[] {
  return Array.isArray(rows) && rows.length <= 100 && rows.every(r => r && typeof r.root_turn_id === 'string' && r.root_turn_id.length <= 512 &&
    Number.isSafeInteger(r.user_round) && Number.isSafeInteger(r.revision) && typeof r.status === 'string' &&
    ['user','assistant'].every(k => r[k] && typeof r[k].content === 'string' && r[k].content.length <= 8192 && typeof r[k].item_id === 'string'));
}
export function threadViewAtLatest(key:string):boolean {void threadViewClock.value;return entries.get(key)?.atLatest ?? true;}
export function threadViewHasNewerPage(key: string, page: ThreadViewPage, generation: number): boolean {
  const e = entries.get(key);
  return Boolean(e && e.generation === generation && ['ready', 'verified_empty'].includes(e.phase) &&
    page && page.session_id === key && Number.isSafeInteger(page.view_seq) && validRows(page.chat) &&
    ['ready', 'source'].includes(page.projection_state) && !page.degraded &&
    e.epoch === page.epoch && e.seq > page.view_seq);
}
export function commitThreadView(key: string, page: ThreadViewPage, generation?: number, historical = false, recovery=false, forward=false): boolean {
  const e = entry(key);
  if (generation !== undefined && e.generation !== generation) return false;
  if(!page || page.session_id!==key || !Number.isSafeInteger(page.epoch) || !Number.isSafeInteger(page.view_seq) || !validRows(page.chat))return false;
  if((historical || forward) && e.epoch!==page.epoch)return false;
  if (!historical && !forward && e.epoch === page.epoch && page.view_seq < e.seq) return false;
  if (page.degraded || page.projection_state === 'degraded') {
    e.error = page.diagnostic === 'view_timeout' ? '线程内容正在整理，已保留当前可见记录' : '线程展示暂时降级';
    e.phase = e.rows.size ? 'ready' : 'opening';
    notify();
    return true;
  }
  if (!['ready','source'].includes(page.projection_state)) { e.error = '线程展示暂时不可用，请重试'; e.phase = e.rows.size ? 'ready' : 'error'; notify(); return false; }
  // Candidate validation precedes every mutation; failures retain committed rows.
  if (!historical && !forward && !(recovery && !e.atLatest && e.epoch===page.epoch)) {
    // Commit a complete tail window atomically. Optimistic requests survive refresh.
    const covered=new Set(page.chat.map(r=>r.root_turn_id));
    for(const id of e.order)if(!id.startsWith('pending:')&&!covered.has(id)){e.rows.delete(id);e.slots.delete(id);}
    e.order=e.order.filter(id=>e.rows.has(id));e.atLatest=true;
    if(e.epoch && e.epoch!==page.epoch){e.rows.clear();e.slots.clear();e.order=[];}
    e.latestRound=page.latest_round;e.latestStatus=page.runtime_status??page.chat.at(-1)?.status??'idle';
  }
  for (const row of page.chat)if(!recovery||e.atLatest||e.rows.has(row.root_turn_id))merge(e,row);
  if (!historical && !forward) { e.seq=page.view_seq;e.epoch=page.epoch; }
  if(!historical && page.runtime_status)e.latestStatus=page.runtime_status;
  if (forward) e.atLatest = !page.has_newer;
  else if(!recovery||e.atLatest){e.next=page.next_before;e.more=page.has_more;}
  e.phase=page.thread_empty === true && !e.rows.size ? 'verified_empty' : 'ready';
  bound(e,historical);notify();return true;
}
export function applyThreadViewChanges(key: string, payload: { runtime_status?:string;active_turn_id?:string;epoch?:number; view_seq: number; reset_required?: boolean; rows: ChatViewRow[] }): boolean {
  const e=entry(key);if(!payload || payload.reset_required || !Number.isSafeInteger(payload.view_seq) || !validRows(payload.rows) || (payload.epoch!==undefined && e.epoch && e.epoch!==payload.epoch)) return false;
  for(const row of payload.rows) {
    if(row.user_round>=e.latestRound){e.latestRound=row.user_round;e.latestStatus=row.status;}
    if(e.atLatest||e.rows.has(row.root_turn_id)||e.rows.has(`pending:${row.client_message_id}`))merge(e,row);
  }
  if(payload.runtime_status && payload.view_seq>=e.seq)e.latestStatus=payload.runtime_status;
  e.seq=Math.max(e.seq,payload.view_seq);if(e.rows.size)e.phase='ready';bound(e);notify();return true;
}
export function failThreadView(key: string, generation: number, error: unknown) { const e=entry(key);if(e.generation!==generation)return;e.error=String(error);e.phase=e.rows.size?'ready':'error';notify(); }
export function threadViewError(key:string):string {void threadViewClock.value;return entries.get(key)?.error ?? '';}
export function threadViewSlots(key:string):ViewSlot[] | null {void threadViewClock.value;const e=entries.get(key);return e ? e.order.map(id=>e.slots.get(id)!).filter(Boolean) : null;}
/**
 * True once the display window is the authoritative render source.
 *
 * `beginThreadView` creates the entry (phase `opening`) before the bootstrap
 * request resolves, so a view entry on its own proves nothing: an open that is
 * still in flight, or one that failed, leaves zero rows. Until a page commits
 * (rows present, or the server declared the thread ready/empty) the durable
 * thread state still owns the transcript, so durable frames must keep applying
 * and durable turn rows must keep rendering. Treating the bare entry as
 * authoritative silently starved that pipeline: the reconnect `text_block`
 * that heals a dropped tail never reached the state, and the page went blank.
 */
export function threadViewCommitted(key:string):boolean {
  void threadViewClock.value;
  const e=entries.get(key);
  return Boolean(e && (e.order.length > 0 || e.phase === 'ready' || e.phase === 'verified_empty'));
}
export function submitThreadView(key:string,client:string,content:string) { const e=entry(key);const id=`pending:${client}`;if(e.rows.has(id))return;
  merge(e,{root_turn_id:id,user_round:Math.max(e.latestRound,...[...e.rows.values()].map(r=>r.user_round))+1,client_message_id:client,revision:-1,status:'running',user:{item_id:'',content},assistant:{item_id:'',content:''},workflow_count:0,failed_count:0});e.phase='ready';bound(e);notify(); }
export function resetThreadViews(){entries.clear();notify();}
export function clearThreadView(key:string) {entries.delete(key);notify();}
export function threadViewStatus(key:string):string|null {
  void threadViewClock.value;const e=entries.get(key);if(!e)return null;
  const rows=e.order.map(id=>e.rows.get(id)!);
  return rows.find(r=>r.root_turn_id.startsWith('pending:')&&['running','queued','waiting_input','waiting_approval'].includes(r.status))?.status??e.latestStatus;
}
export function settlePendingThreadView(key:string,client:string,status:string,detail='') {
  const e=entries.get(key);const id=`pending:${client}`;const r=e?.rows.get(id);if(!e||!r)return;
  r.status=status;r.assistant={...r.assistant,content:detail};e.slots.set(id,materialize(r));notify();
}
export function removePendingThreadView(key:string,client:string) {const e=entries.get(key);if(!e)return;const id=`pending:${client}`;e.rows.delete(id);e.slots.delete(id);e.order=e.order.filter(v=>v!==id);notify();}
/** Remove a queued turn after the optimistic row has already been bound to a
 * durable root_turn_id. Queue withdrawal must address both identities because
 * the binding can race the cancel response. */
export function removeThreadViewTurnByClientMessageId(key:string,client:string) {
  const e = entries.get(key);
  const cleaned = String(client || '').trim();
  if (!e || !cleaned) return;
  const removed = new Set<string>();
  for (const [id, row] of e.rows) {
    if (String(row.client_message_id || '').trim() === cleaned || id === `pending:${cleaned}`) {
      removed.add(id);
      e.rows.delete(id);
      e.slots.delete(id);
    }
  }
  if (!removed.size) return;
  e.order = e.order.filter((id) => !removed.has(id));
  e.bytes = e.order.reduce((n,id) => {
    const row = e.rows.get(id);
    return n + (row ? 2 * (row.user.content.length + row.assistant.content.length + (row.assistant.reasoning?.length ?? 0) + (row.assistant.tail?.length ?? 0)) + 4096 : 0);
  }, 0);
  notify();
}
export function applyThreadViewTail(key:string,payload:any):boolean {
  const e=entries.get(key);if(!e)return false;
  const p=payload?.data??payload;const id=String(p?.item_id??'');const delta=String(p?.text??p?.delta??p?.content??'');
  if(!id||!delta)return true;
  for(const row of e.rows.values()) {
    if(row.assistant.item_id!==id)continue;
    if(!['running','queued','waiting_input','waiting_approval'].includes(row.status))return true;
    if (String(p?.field || 'content') === 'reasoning') {
      // Reasoning is a bounded live projection (the durable item remains the
      // source of truth). Keep enough text for the expanded process entry and
      // preserve the stream when the server preview has already been capped.
      const current = String(row.assistant.reasoning || '');
      const offset = Number(p.reasoning_offset ?? p.offset);
      const cursor = Number(row.assistant.reasoning_offset || 0);
      const expected = Number.isSafeInteger(offset) && offset >= 0 ? offset : cursor;
      if (!Number.isSafeInteger(expected) || expected < 0 || expected !== cursor) return true;
      const next = current + delta;
      const bounded = next.length > 32768 ? next.slice(-32768) : next;
      row.assistant = { ...row.assistant, reasoning: bounded,
        reasoning_offset: expected + [...delta].reduce((n, ch) => n + ch.length, 0) };
      e.slots.set(row.root_turn_id, materialize(row));
      notifyTail();return true;
    }
    const offset=Number(p.content_offset??p.offset);
    const base=Number(p.base_seq??0);
    if(base>(row.assistant.durable_seq??0))return true;
    const expected=row.assistant.content_offset??row.assistant.content.length;
    // Do not invent a contiguous answer across lost, duplicated or reordered tails.
    // The next durable block supplies an authoritative prefix/tail and offset.
    if(!Number.isSafeInteger(offset)||offset!==expected)return true;
    const encoder=new TextEncoder();
    const prefix=(s:string)=>{let out='',bytes=0;for(const ch of s){const n=encoder.encode(ch).length;if(bytes+n>2048)break;out+=ch;bytes+=n;}return out;};
    if(offset===row.assistant.content.length && encoder.encode(row.assistant.content).length<2048) {
      row.assistant={...row.assistant,content:prefix(row.assistant.content+delta)};
    } else {
      const tail=(row.assistant.tail??'')+delta;let out='',bytes=0;for(const ch of [...tail].reverse()){const n=encoder.encode(ch).length;if(bytes+n>2048)break;out=ch+out;bytes+=n;}
      row.assistant={...row.assistant,tail:out};
    }
    const deltaUnits = [...delta].reduce((n, ch) => n + ch.length, 0);
    row.assistant={...row.assistant,content_offset:expected+deltaUnits,
      tail_offset: expected + deltaUnits,
      body:row.assistant.body?{...row.assistant.body,bytes:row.assistant.body.bytes+encoder.encode(delta).length}:undefined};
    e.slots.set(row.root_turn_id,materialize(row));notifyTail();return true;
  }
  return true;
}
