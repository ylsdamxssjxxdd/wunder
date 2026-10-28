// AI生成
// The display window is independent of connection and execution-log state.
import { shallowRef, markRaw } from 'vue';
import type { ProcessActivity } from '@/components/chat/timelineActivity';
import { reconcileChatQueueRows } from '@/stores/chatQueueLiveState';
import { cancelThreadBodyHydration, enqueueThreadBodyHydration, resetThreadBodyHydration } from './threadBodyHydration';
import type { ChatPlan } from './chatPlan';
import { presentChatInquiry, type ChatInquiryPanel } from './chatInquiry';
import { reconcileThreadStream, LIVE_STREAM_MAX_CHARS } from './threadViewStream';

export interface ThreadBodyRef { body_id: string; revision: number; bytes: number }
export interface ViewMessage { item_id: string; content: string; reasoning?: string; created_seq?: number; durable_seq?: number; content_offset?: number; reasoning_offset?: number; tail_offset?: number; body?: ThreadBodyRef; feedback?: unknown; stats?: unknown; tail?: string; failure?: string; attachments?: unknown[]; /** Full body text once the lazy read landed, plus the identity it belongs to. */ hydrated_content?: string; hydrated_body?: string; /** Contiguous local view of a stream; the durable row drops its middle. */ live_content?: string; live_offset?: number; question_panel?: ChatInquiryPanel | null }
export interface ChatViewRow { root_turn_id: string; execution_turn_id?:string; user_round: number; client_message_id?: string; revision: number; status: string; stop_reason?: string | null; queued_input?: { queue_id: string; reason: string; priority: number } | null; user: ViewMessage; assistant: ViewMessage; workflow_count: number; process_count?: number | null; process_items?: Array<Record<string, any>>; process_revision?: number; workspace_change_seq?: number; reasoning_count?: number; reasoning_item_id?: string; process_activity?: ProcessActivity; failed_count: number; subagents?: any[]; plan?: ChatPlan | null }
export interface ThreadViewPage { runtime_status?:string;status_turn_id?:string;active_turn_id?:string; session_id: string; epoch: number; view_seq: number; projection_state: string; degraded?: boolean; diagnostic?: string | null; thread_empty: boolean | null; latest_round: number; chat: ChatViewRow[]; trajectory: any[]; tiles: any[]; tile_level: number; next_before: number | null; has_more: boolean; next_after?: number | null; has_newer?: boolean }
export type ViewPhase = 'opening' | 'ready' | 'verified_empty' | 'refreshing' | 'error';
export interface ViewSlot { key: string; rootTurnId: string; user: Record<string, any>; assistant: Record<string, any> }
interface RowExecutionHead { turnId: string; status: string; seq: number }
interface Entry { key: string; phase: ViewPhase; epoch: number; seq: number; generation: number; rows: Map<string, ChatViewRow>; slots: Map<string, ViewSlot>; rowHeads: Map<string, RowExecutionHead>; order: string[]; next: number | null; more: boolean; bytes: number; error: string; atLatest: boolean; latestRound: number; latestStatus: string; headSeq: number; activeTurnId: string; statusTurnId: string; headAuthoritative: boolean; submissionHead?: { id: string; seq: number } }
const entries = new Map<string, Entry>();
export const threadViewClock = shallowRef(0);
const MAX_BYTES = 48 * 1024 * 1024;
const MAX_ROWS = 100;
let activeKey = '';
let nextGeneration = 0;
function entry(key: string): Entry {
  let e = entries.get(key);
  if (!e) { e = { key, phase: 'opening', epoch: 0, seq: 0, generation: 0, rows: new Map(), slots: new Map(), rowHeads: new Map(), order: [], next: null, more: false, bytes: 0, error: '', atLatest: true, latestRound: 0, latestStatus: 'idle', headSeq: -1, activeTurnId: '', statusTurnId: '', headAuthoritative: false }; entries.set(key, e); }
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
  const running = ['accepted','running','queued','waiting_input','waiting_approval'].includes(row.status);
  const onlineSteps = (row.process_items || []).reduce((total, item) => total + (item.kind === 'assistant_message'
    ? Number(Boolean(String(item.payload?.reasoning || '').trim())) + Number(Boolean(String(item.payload?.content || '').trim()) &&
      (item.item_id !== row.assistant.item_id || item.payload?.tool_calls?.length > 0))
    : 1), 0);
  const make = (role: string, m: ViewMessage) => markRaw({ id: `tturn:${row.root_turn_id}:${role}`, role, content: m.content, item_id: m.item_id,
    turn_id: row.execution_turn_id??row.root_turn_id, root_turn_id: row.root_turn_id, user_round: row.user_round, client_message_id: row.client_message_id,
    __runtime_projected: true, __thread_view:true, final: !running, state:role==='assistant'?(running?'streaming':row.status==='completed'?'final':row.status):'final',
    // Only a still-truncated body needs an explicit read; copy falls back to it.
    __thread_body: Boolean(m.body && m.body.bytes > new TextEncoder().encode(m.content).length) ? m.body : undefined,
    streaming: role === 'assistant' && running, status: role === 'assistant' ? row.status : 'completed',
    stop_reason: row.stop_reason, queued_input: row.queued_input, workspace_change_seq: row.workspace_change_seq ?? 0,
    // The process badge counts every visible process entry.  The server keeps
    // tool counts separately, so include the aggregated reasoning entry here.
    workflow_count: row.workflow_count, reasoning_count: Number(row.reasoning_count || 0),
    // The bounded chat row and the expanded process endpoint are two views of
    // the same stable assistant item. Older servers may omit
    // `reasoning_item_id`; use the assistant item identity whenever the row
    // carries reasoning so a truncated preview still binds to its full
    // process record instead of rendering a duplicate thought.
    reasoning_item_id: row.reasoning_item_id || (String(m.reasoning || '').trim() ? m.item_id : ''),
    process_count: Math.max(onlineSteps, row.process_count ?? (row.workflow_count + Number(row.reasoning_count || 0) + (role === 'assistant' && String(m.reasoning || '').trim() && !Number(row.reasoning_count || 0) ? 1 : 0))), workflow_failed_count: row.failed_count, feedback: m.feedback, stats: m.stats, reasoning: m.reasoning || '',
    process_activity: row.process_activity?.kind ? row.process_activity : undefined,
    process_items: row.process_items, process_revision: row.process_revision,
    process_refresh_revision: row.revision,
    __thread_tail: m.tail, failureDetail:m.failure, attachments:m.attachments,
    __thread_tail_offset: m.tail_offset, __thread_content_offset: m.content_offset,
    __thread_live: m.live_content, __thread_live_offset: m.live_offset,
    workflowItems: [], ...(role==='assistant'?{plan:row.plan ?? null,questionPanel:presentChatInquiry(m.question_panel,row.status)}:{}), ...(role==='assistant'&&Array.isArray(row.subagents)&&row.subagents.length>0?{subagents:row.subagents}:{}), failed: role === 'assistant' && ['failed','rejected'].includes(row.status), stopped: row.status === 'cancelled' });
  return markRaw({ key: row.client_message_id ? `turn:client:${row.client_message_id}` : `turn:${row.root_turn_id}`, rootTurnId: row.root_turn_id, user: make('user', row.user), assistant: make('assistant', row.assistant) });
}
const isQueueOnlyCancelledRow = (row: ChatViewRow): boolean =>
  row.status === 'cancelled' &&
  !String(row.assistant?.item_id || '').trim() &&
  !String(row.assistant?.content || '').trim() &&
  !String(row.assistant?.reasoning || '').trim() &&
  Number(row.workflow_count || 0) === 0;
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
/**
 * Projection rows carry a bounded preview plus a `BodyRef`. The durable body is
 * read lazily and written back into the row, so every answer renders through
 * the ordinary Markdown pipeline (code blocks, tables, workspace links) instead
 * of collapsing into plain text behind an expand control.
 */
export type BodyRole = 'user' | 'assistant';
const LIVE_ROW_STATUSES = ['accepted','running','queued','waiting_input','waiting_approval'];
const bodyToken = (message: ViewMessage | undefined): string =>
  message?.body ? `${message.body.body_id}@${message.body.revision}` : '';
const bodyBytes = (text: string): number => new TextEncoder().encode(text).length;
/** A streamed answer has two local extensions: the contiguous
 *  text (`live_content`) and the lazy durable body (`hydrated_content`).
 *  Both live outside the server's bounded preview and must survive later
 *  pushes of the same row. */
function keepLocalBody(previous: ViewMessage | undefined, next: ViewMessage): ViewMessage {
  if (!previous) return next;
  let out = next;
  const token = bodyToken(next);
  const hydrated = previous.hydrated_content;
  if (hydrated && token && previous.hydrated_body === token &&
      String(next.content || '').length < hydrated.length) {
    const units = utf16Units(hydrated);
    out = { ...out, content: hydrated, content_offset: units, tail: '', tail_offset: units,
      hydrated_content: hydrated, hydrated_body: token };
  }
  // Only the same item continues the same stream; a new item starts empty.
  if (previous.item_id === next.item_id) {
    out = { ...out, ...reconcileThreadStream(previous, out) };
  }
  return out;
}
/** Reset the local stream view once the durable body is authoritative. */
function clearLiveStream(message: ViewMessage): ViewMessage {
  return { ...message, live_content: '', live_offset: undefined, tail: '', tail_offset: utf16Units(message.content) };
}
function scheduleBodyHydration(e: Entry, id: string, role: BodyRole): void {
  const row = e.rows.get(id);
  const message = row?.[role];
  if (!row || !message?.body) return;
  // Only an assistant answer has a live window; a user input is complete the
  // moment its body exists, so the stream guards must not skip its read.
  if (role === 'assistant' && LIVE_ROW_STATUSES.includes(row.status)) {
    const retained = String(message.live_content || message.content || '').length;
    // Recover missed durable windows once, through the bounded body reader.
    if (retained >= LIVE_STREAM_MAX_CHARS - 1 || retained >= Number(message.content_offset || 0)) return;
  }
  if (message.body.bytes <= bodyBytes(String(message.content || ''))) return;
  const token = `${bodyToken(message)}:${role}:${id}`;
  void enqueueThreadBodyHydration({
    sessionId: e.key,
    bodyId: message.body.body_id,
    revision: message.body.revision,
    token
  }).then((content) => applyBodyHydration(e.key, id, role, token, content));
}
/** Write-back of a lazy body read; exported for the store regression fixtures. */
export function applyBodyHydration(key: string, id: string, role: BodyRole, expected: string, content: string | null): void {
  if (!content) return;
  const e = entries.get(key);
  const row = e?.rows.get(id);
  if (!e || !row) return;
  const message = row[role];
  // A newer revision or another body already replaced the read target.
  if (!message?.body || `${bodyToken(message)}:${role}:${id}` !== expected) return;
  const units = utf16Units(content);
  // A running turn reconciles the assistant stream around its bounded preview.
  // User input never streams: routing it through that join drops the read text
  // and leaves the bubble at the 2 KB preview until the turn settles.
  if (role === 'assistant' && LIVE_ROW_STATUSES.includes(row.status)) {
    const retained = String(message.live_content || '');
    const live = reconcileThreadStream({ content: retained.length > content.length ? retained : content }, message);
    row[role] = { ...message, ...live };
    e.slots.set(id, materialize(row));
    notify();
    return;
  }
  row[role] = { ...clearLiveStream(message), content, content_offset: units, tail: '', tail_offset: units,
    hydrated_content: content, hydrated_body: bodyToken(message) };
  e.slots.set(id, materialize(row));
  notify();
}
/**
 * The durable row keeps a 2 KB head plus a 2 KB tail window, so the middle of a
 * long answer disappears as the stream advances — the body shrinks mid-output
 * and grows again. Accumulating deltas locally restores one contiguous text.
 * A gap in the offset sequence keeps the readable prefix until a bounded body
 * read fills the missing range.
 */
function accumulateLiveStream(message: ViewMessage, delta: string, expected: number, deltaUnits: number): Partial<ViewMessage> {
  return reconcileThreadStream(message, {
    content: message.content, tail: delta, tail_offset: expected + deltaUnits
  });
}
function carryLocalBody(row: ChatViewRow, previous: ChatViewRow | undefined): ChatViewRow {
  if (!previous) return row;
  // The durable row never carries local accumulation; re-attach it per role.
  return { ...row, user: keepLocalBody(previous.user, row.user), assistant: keepLocalBody(previous.assistant, row.assistant) };
}
function projectRowExecutionHead(e: Entry, row: ChatViewRow): ChatViewRow {
  const turnId = row.execution_turn_id || row.root_turn_id;
  let head = e.rowHeads.get(row.root_turn_id);
  if (e.headAuthoritative && e.statusTurnId === turnId && row.revision <= e.headSeq) {
    head = { turnId, status: e.latestStatus, seq: e.headSeq };
    e.rowHeads.set(row.root_turn_id, head);
  }
  if (!head) return row;
  if (head.turnId !== turnId || row.revision > head.seq) {
    e.rowHeads.delete(row.root_turn_id);
    return row;
  }
  return row.status === head.status ? row : { ...row, status: head.status };
}
function merge(e: Entry, row: ChatViewRow) {
  const old = e.rows.get(row.root_turn_id);
  if (old && old.revision >= row.revision) return;
  row = projectRowExecutionHead(e, row);
  // Acceptance binds identity; dispatch decides ownership of parked input.
  const previous = old ?? e.rows.get(`pending:${row.client_message_id}`);
  if (previous?.process_items && (!row.execution_turn_id || row.execution_turn_id === previous.execution_turn_id)) {
    row = { ...row, process_items: previous.process_items, process_revision: previous.process_revision };
  }
  if (row.status === 'accepted' && previous?.status === 'queued') row={...row,status:'queued'};
  // A display refresh can be published between stream_started and the first
  // durable assistant item. Its assistant slot is intentionally empty; it is
  // not evidence that the live tail stopped. Keep the item-bound local stream
  // until a durable assistant identity replaces it.
  if (old && LIVE_ROW_STATUSES.includes(row.status) &&
      String(old.assistant.item_id || '').trim() && (!String(row.assistant.item_id || '').trim() ||
        (tailTurnId(old.assistant.item_id) === tailTurnId(row.assistant.item_id) &&
          modelRound(old.assistant.item_id) > modelRound(row.assistant.item_id)))) {
    row = {
      ...row,
      assistant: {
        ...row.assistant,
        item_id: old.assistant.item_id,
        content: old.assistant.content,
        reasoning: old.assistant.reasoning,
        content_offset: old.assistant.content_offset,
        reasoning_offset: old.assistant.reasoning_offset,
        tail: old.assistant.tail,
        tail_offset: old.assistant.tail_offset,
        live_content: old.assistant.live_content,
        live_offset: old.assistant.live_offset,
        body: old.assistant.body
      },
      reasoning_count: Math.max(Number(row.reasoning_count || 0), Number(old.reasoning_count || 0)),
      process_count: row.process_count == null && old.process_count == null ? undefined : Math.max(Number(row.process_count || 0), Number(old.process_count || 0)),
      reasoning_item_id: old.reasoning_item_id || row.reasoning_item_id,
      process_activity: old.process_activity ?? row.process_activity
    };
  }
  if(old && old.assistant.item_id===row.assistant.item_id && ['accepted','running','queued','waiting_input','waiting_approval'].includes(row.status)) {
    const previous = old.process_activity;
    const incoming = row.process_activity;
    if (previous && (!incoming || Number(previous.created_seq || 0) > Number(incoming.created_seq || 0) ||
        (previous.item_id === incoming.item_id && previous.kind === 'responding' && incoming.kind === 'thinking'))) {
      row = { ...row, process_activity: previous };
    }
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
      if (keepOldReasoning) row = { ...row, reasoning_item_id: old.reasoning_item_id || old.assistant.item_id };
    }
  }
  row = carryLocalBody(row, previous);
  if (LIVE_ROW_STATUSES.includes(row.status)) {
    row = { ...row, assistant: { ...row.assistant, ...reconcileThreadStream(row.assistant, row.assistant) } };
  }
  e.rows.set(row.root_turn_id, markRaw(row)); e.slots.set(row.root_turn_id, materialize(row));
  scheduleBodyHydration(e, row.root_turn_id, 'assistant');
  scheduleBodyHydration(e, row.root_turn_id, 'user');
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
  for (const id of e.rowHeads.keys()) if (!e.rows.has(id)) e.rowHeads.delete(id);
  // The other edge was evicted during forward navigation or a live append.
  // Its next read must start at the retained boundary, not the old page cursor.
  if ((evictsOlder || e.next === null) && e.order.length) e.next = e.rows.get(e.order[0])!.user_round;
  e.bytes = e.order.reduce((n,id) => { const r=e.rows.get(id)!; return n+2*(r.user.content.length+r.assistant.content.length+(r.assistant.reasoning?.length??0)+(r.assistant.tail?.length??0))+4096+(r.process_items?.length??0)*32768; }, 0);
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
/** State and transcript cursors are independent: old row revisions may fill
 * holes, but only a monotonic envelope can change the execution head. */
function applyExecutionHead(e: Entry, payload: {view_seq:number;runtime_status?:string;status_turn_id?:string;active_turn_id?:string}, rows: ChatViewRow[]) {
  if (payload.view_seq < e.headSeq) return;
  if (!payload.runtime_status && e.headAuthoritative) return;
  if (payload.runtime_status) {
    e.latestStatus=payload.runtime_status;
    e.statusTurnId=String(payload.status_turn_id || payload.active_turn_id || '');
    e.activeTurnId=String(payload.active_turn_id || '');
    e.headAuthoritative=true;
  } else if (!e.headAuthoritative) {
    // Source fixtures/legacy reads have no head. Once supplied, rows never own it again.
    const active=rows.find(r=>['running','waiting_input','waiting_approval'].includes(r.status))
      ?? rows.find(r=>r.status==='accepted') ?? rows.at(-1);
    if(active) {e.latestStatus=active.status==='accepted'?'running':active.status;e.activeTurnId=active.execution_turn_id || active.root_turn_id;e.statusTurnId=e.activeTurnId;}
  }
  e.headSeq=payload.view_seq;
  for (const row of e.rows.values()) {
    const projected = projectRowExecutionHead(e, row);
    if (projected === row) continue;
    e.rows.set(row.root_turn_id, markRaw(projected));
    e.slots.set(row.root_turn_id, materialize(projected));
  }
}
const optimisticSubmissionOwnsExecutionHead = (e: Entry): boolean => {
  // Bridge only the terminal head observed when this submission was made.
  // Any newer server head takes ownership, even if the local placeholder has
  // not been bound yet. A leftover pending row must never reopen a later
  // completion or turn an input waiting in the composer into an execution.
  const submission = e.submissionHead;
  return Boolean(submission && e.headSeq === submission.seq &&
    e.rows.get(submission.id)?.status === 'accepted');
};

export function threadViewActiveTurnId(key:string):string {
  void threadViewClock.value;
  const e=entries.get(key);
  if (!e) return '';
  if (optimisticSubmissionOwnsExecutionHead(e)) {
    return e.submissionHead!.id;
  }
  return e.activeTurnId || '';
}
export function threadViewOwnsExecution(key:string):boolean {void threadViewClock.value;return entries.get(key)?.headAuthoritative ?? false;}
/** Admission guards compare execution watermarks, never transcript cursors. */
export function threadViewHeadVersion(key:string):string {
  void threadViewClock.value;
  const e=entries.get(key);
  return e?.headAuthoritative ? `${e.epoch}:${e.headSeq}` : '';
}
/** Catalog reads can advance detached thread status without advancing the
 * transcript replay cursor (otherwise reconnect would skip missing rows). */
export function applyThreadViewHead(key:string, head:any):boolean {
  const e=entries.get(key);
  if(!e || !head || head.epoch!==e.epoch || !Number.isSafeInteger(head.view_seq) ||
      !head.runtime_status || head.view_seq<=e.headSeq)return false;
  applyExecutionHead(e,head,[]);
  notify();return true;
}

export function commitThreadView(key: string, page: ThreadViewPage, generation?: number, historical = false, recovery=false, forward=false): boolean {
  const e = entry(key);
  if (generation !== undefined && e.generation !== generation) return false;
  if(!page || page.session_id!==key || !Number.isSafeInteger(page.epoch) || !Number.isSafeInteger(page.view_seq) || !validRows(page.chat))return false;
  // Source-log fallback can only advance into the presentation epoch. A late
  // fallback HTTP response cannot switch an already live projection back.
  if (e.headAuthoritative && e.epoch > 0 && page.epoch === -1) return false;
  const receivedRows=page.chat;
  page = { ...page, chat: page.chat.filter(row => !isQueueOnlyCancelledRow(row)) };
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
    if(e.epoch && e.epoch!==page.epoch){e.rows.clear();e.slots.clear();e.rowHeads.clear();e.order=[];e.headSeq=-1;e.headAuthoritative=false;e.activeTurnId='';e.statusTurnId='';}
    e.latestRound=Math.max(e.latestRound,page.latest_round);
  }
  for (const row of page.chat)if(!recovery||e.atLatest||e.rows.has(row.root_turn_id))merge(e,row);
  reconcileChatQueueRows(key, receivedRows, page.epoch);
  if (!historical && !forward) { e.seq=page.view_seq;e.epoch=page.epoch; }
  if(!historical && !forward) applyExecutionHead(e, page, page.chat);
  if (forward) e.atLatest = !page.has_newer;
  else if(!recovery||e.atLatest){e.next=page.next_before;e.more=page.has_more;}
  e.phase=page.thread_empty === true && !e.rows.size ? 'verified_empty' : 'ready';
  bound(e,historical);notify();return true;
}
export function applyThreadViewChanges(key: string, payload: { runtime_status?:string;status_turn_id?:string;active_turn_id?:string;epoch?:number; view_seq: number; reset_required?: boolean; rows: ChatViewRow[] }): boolean {
  const e=entry(key);if(!payload || payload.reset_required || !Number.isSafeInteger(payload.view_seq) || !validRows(payload.rows) || (payload.epoch!==undefined && e.epoch && e.epoch!==payload.epoch)) return false;
  for(const row of payload.rows.filter(row => !isQueueOnlyCancelledRow(row))) {
    e.latestRound=Math.max(e.latestRound,row.user_round);
    if(e.atLatest||e.rows.has(row.root_turn_id)||e.rows.has(`pending:${row.client_message_id}`))merge(e,row);
  }
  applyExecutionHead(e, payload, payload.rows);
  if (!e.epoch && payload.epoch !== undefined) e.epoch=payload.epoch;
  reconcileChatQueueRows(key, payload.rows, payload.epoch);
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
 * authoritative silently starved that pipeline: the reconnect completed item
 * that heals a dropped tail never reached the state, and the page went blank.
 */
export function threadViewCommitted(key:string):boolean {
  void threadViewClock.value;
  const e=entries.get(key);
  return Boolean(e && (e.order.length > 0 || e.phase === 'ready' || e.phase === 'verified_empty'));
}
export function submitThreadView(key:string,client:string,content:string) { const e=entry(key);const id=`pending:${client}`;if(e.rows.has(id))return;
  const queued = ['accepted','running','queued','waiting_input','waiting_approval'].includes(threadViewStatus(key) || '');
  if (!queued) e.submissionHead = { id, seq: e.headSeq };
  merge(e,{root_turn_id:id,user_round:Math.max(e.latestRound,...[...e.rows.values()].map(r=>r.user_round))+1,client_message_id:client,revision:-1,status:queued?'queued':'accepted',user:{item_id:'',content},assistant:{item_id:'',content:''},workflow_count:0,failed_count:0});e.phase='ready';bound(e);notify(); }
/** Bind the optimistic user turn before the first item tail.  The websocket
 * acknowledgement normally arrives ahead of the display projection; without
 * this small identity hand-off a `turn:text-N` tail cannot find the pending
 * row and the user sees the thought only after its durable flush. */
export function bindThreadViewStreamStarted(key: string, data: { turn_id?: unknown; client_message_id?: unknown; user_round?: unknown }): boolean {
  const e = entries.get(key);
  const turnId = String(data?.turn_id || '').trim();
  if (!e || !turnId) return false;
  const client = String(data?.client_message_id || '').trim();
  let oldId = client ? `pending:${client}` : '';
  let row = oldId ? e.rows.get(oldId) : undefined;
  if (!row && client) {
    for (const [id, candidate] of e.rows) {
      if (id.startsWith('pending:') && String(candidate.client_message_id || '') === client) {
        oldId = id;
        row = candidate;
        break;
      }
    }
  }
  if (!row || oldId === turnId || e.rows.has(turnId)) return false;
  const userRound = Number(data?.user_round);
  const bound: ChatViewRow = markRaw({
    ...row,
    root_turn_id: turnId,
    execution_turn_id: turnId,
    user_round: Number.isSafeInteger(userRound) && userRound > 0 ? userRound : row.user_round
  });
  e.rows.delete(oldId);
  e.slots.delete(oldId);
  e.rows.set(turnId, bound);
  e.slots.set(turnId, materialize(bound));
  e.order = e.order.map((id) => id === oldId ? turnId : id);
  if (e.submissionHead?.id === oldId) e.submissionHead = { ...e.submissionHead, id: turnId };
  notify();
  return true;
}
export function resetThreadViews(){entries.clear();resetThreadBodyHydration();notify();}
export function clearThreadView(key:string) {entries.delete(key);cancelThreadBodyHydration(key);notify();}
export function threadViewStatus(key:string):string|null {
  void threadViewClock.value;const e=entries.get(key);if(!e)return null;
  // A fresh submission bridges its previously observed head only. Later
  // server heads stay authoritative even if a local pending row remains.
  if (e.headAuthoritative) return optimisticSubmissionOwnsExecutionHead(e) ? 'running' : e.latestStatus;
  const rows=e.order.map(id=>e.rows.get(id)!);
  // Before the first durable head arrives, keep the optimistic submission
  // responsive by treating an accepted local row as a provisional run.
  if (['running','waiting_input','waiting_approval'].includes(e.latestStatus)) return e.latestStatus;
  return rows.find(r=>r.root_turn_id.startsWith('pending:')&&r.status==='accepted')?'running':e.latestStatus;
}
export function settlePendingThreadView(key:string,client:string,status:string,detail='') {
  const e=entries.get(key);const id=`pending:${client}`;const r=e?.rows.get(id);if(!e||!r)return;
  r.status=status;r.assistant={...r.assistant,content:detail};e.slots.set(id,materialize(r));notify();
}
export function removePendingThreadView(key:string,client:string) {const e=entries.get(key);if(!e)return;const id=`pending:${client}`;e.rows.delete(id);e.slots.delete(id);e.order=e.order.filter(v=>v!==id);notify();}
/** Remove a locally parked turn after a confirmed queue withdrawal.  A client
 * message id is not itself permission to remove a durable conversation turn:
 * a queue start may win the cancel race and bind that same id to real output. */
export function removeThreadViewTurnByClientMessageId(key:string,client:string) {
  const e = entries.get(key);
  const cleaned = String(client || '').trim();
  if (!e || !cleaned) return;
  const removed = new Set<string>();
  for (const [id, row] of e.rows) {
    const locallyPending = id === `pending:${cleaned}`;
    const durablyParked = String(row.client_message_id || '').trim() === cleaned &&
      row.status === 'queued';
    if (locallyPending || durablyParked) {
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
function advanceTailActivity(row: ChatViewRow, kind: 'thinking' | 'responding'): void {
  const current = row.process_activity;
  const createdSeq = Number(row.assistant.created_seq || 0);
  // A delayed tail may still extend its model item after a newer tool starts.
  // It must not switch the process header back to that older model phase.
  if (Number(current?.created_seq || 0) > createdSeq ||
    (!createdSeq && current?.item_id && current.item_id !== row.assistant.item_id)) return;
  if (current?.item_id === row.assistant.item_id && current.kind === 'responding' && kind === 'thinking') return;
  row.process_activity = { kind, item_id: row.assistant.item_id, created_seq: createdSeq };
}

/** `thread_item_tail` is emitted before the immutable assistant item exists.
 * The display row is already known from turn admission, but its compact
 * assistant projection deliberately has no item id yet.  Bind that blank
 * projection to the stable streaming id here; otherwise the first thought is
 * dropped until the final item is persisted. */
function tailTurnId(itemId: string): string {
  const marker = itemId.lastIndexOf(':text-');
  return marker > 0 ? itemId.slice(0, marker) : '';
}

function modelRound(itemId: string): number {
  const marker = itemId.lastIndexOf(':text-');
  if (marker < 0) return -1;
  const value = Number(itemId.slice(marker + ':text-'.length));
  return Number.isSafeInteger(value) && value >= 0 ? value : -1;
}

/** Locate the active user-round row for a tail.  An exact item match owns a
 * continuation.  Before the assistant row is durable, use the execution turn
 * encoded by the canonical `turn:text-model_round` identity. */
function tailRow(e: Entry, itemId: string): ChatViewRow | undefined {
  const executionId = tailTurnId(itemId);
  let rootMatch: ChatViewRow | undefined;
  for (const row of e.rows.values()) {
    if (!LIVE_ROW_STATUSES.includes(row.status)) continue;
    if (row.assistant.item_id === itemId) return row;
    if (executionId && row.execution_turn_id === executionId) return row;
    if (executionId && row.root_turn_id === executionId && (!row.execution_turn_id || row.execution_turn_id === executionId)) rootMatch = row;
  }
  if (rootMatch) return rootMatch;
  // Websocket ordering normally supplies stream_started first.  A reconnect
  // or a very fast provider can reverse those two frames, however.  The only
  // locally submitted, unbound active row is then the safe owner of the first
  // tail; queued rows deliberately never qualify.
  const submitted = e.submissionHead?.id ? e.rows.get(e.submissionHead.id) : undefined;
  if (submitted && submitted.root_turn_id.startsWith('pending:') && submitted.status === 'accepted' &&
      !String(submitted.assistant.item_id || '').trim()) return submitted;
  return undefined;
}

export function applyThreadViewProcessItem(key: string, payload: any): boolean {
  const current = entries.get(key);
  if (!current) return false;
  const item = payload?.data ?? payload;
  const id = String(item?.item_id || '');
  const execution = String(item?.turn_id || '');
  const kind = String(item?.kind || '');
  if (!id || !execution || !['tool_call', 'assistant_message', 'compaction'].includes(kind)) return true;
  const row = [...current.rows.values()].find(candidate =>
    (candidate.execution_turn_id || candidate.root_turn_id) === execution && LIVE_ROW_STATUSES.includes(candidate.status));
  if (!row) return true;
  const items = new Map((row.process_items || []).map(previous => [String(previous.item_id), previous]));
  const previous = items.get(id);
  if (previous && previous.status !== 'running' && item.status === 'running') return true;
  items.set(id, item);
  row.process_items = [...items.values()].slice(-100);
  let retained = row.process_items.length;
  for (const other of [...current.rows.values()].reverse()) {
    if (other === row || !other.process_items?.length) continue;
    other.process_items = other.process_items.slice(-Math.max(0, 100 - retained));
    if (retained >= 100) other.process_items = [];
    retained += other.process_items.length;
    current.slots.set(other.root_turn_id, materialize(other));
  }
  row.process_revision = Number(row.process_revision || 0) + 1;
  if (kind === 'tool_call' && item.status === 'running') row.process_activity = {
    kind: 'tool', item_id: id, tool: String(item.payload?.tool || item.payload?.tool_name || ''), created_seq: row.assistant.created_seq
  };
  current.slots.set(row.root_turn_id, materialize(row));
  bound(current);
  notifyTail();
  return true;
}

/** Move a running compact row to a newly observed model item.  A later model
 * round follows tools and its partial prose/reasoning belongs to the work
 * process, not to the previous round's bounded preview. */
function bindTailAssistant(row: ChatViewRow, itemId: string): boolean {
  const current = String(row.assistant.item_id || '');
  if (current === itemId) return true;
  const incomingRound = modelRound(itemId);
  const currentRound = modelRound(current);
  // Never let a delayed earlier model round overwrite a newer compact row.
  if (current && tailTurnId(current) === tailTurnId(itemId) && incomingRound >= 0 && currentRound >= 0 && incomingRound < currentRound) return false;
  row.assistant = {
    ...row.assistant,
    item_id: itemId,
    content: '',
    reasoning: '',
    content_offset: 0,
    reasoning_offset: 0,
    tail: '',
    tail_offset: 0,
    live_content: '',
    live_offset: 0,
    body: undefined
  };
  row.reasoning_item_id = '';
  return true;
}
export function applyThreadViewTail(key:string,payload:any):boolean {
  const e=entries.get(key);if(!e)return false;
  const p=payload?.data??payload;const id=String(p?.item_id??'');const delta=String(p?.text??p?.delta??p?.content??'');
  if(!id||!delta)return true;
  const row = tailRow(e, id);
  if (row) {
    const previousReasoningId = row.reasoning_item_id;
    if (!bindTailAssistant(row, id)) return true;
    if (String(p?.field || 'content') === 'reasoning') {
      if (row.reasoning_item_id && row.reasoning_item_id !== id) {
        row.assistant = { ...row.assistant, reasoning: '', reasoning_offset: 0 };
      }
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
      advanceTailActivity(row, 'thinking');
      row.assistant = { ...row.assistant, reasoning: bounded,
        reasoning_offset: expected + [...delta].reduce((n, ch) => n + ch.length, 0) };
      row.reasoning_item_id = id;
      if (row.process_count != null && previousReasoningId !== id) row.process_count += 1;
      row.reasoning_count = Math.max(1, Number(row.reasoning_count || 0));
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
    const deltaUnits = [...delta].reduce((n, ch) => n + ch.length, 0);
    // Read before the durable head/tail are rewritten: a seed compares the
    // previous head against the offset this frame starts from.
    const live = accumulateLiveStream(row.assistant, delta, expected, deltaUnits);
    const encoder=new TextEncoder();
    const prefix=(s:string)=>{let out='',bytes=0;for(const ch of s){const n=encoder.encode(ch).length;if(bytes+n>2048)break;out+=ch;bytes+=n;}return out;};
    if(offset===row.assistant.content.length && encoder.encode(row.assistant.content).length<2048) {
      row.assistant={...row.assistant,content:prefix(row.assistant.content+delta)};
    } else {
      const tail=(row.assistant.tail??'')+delta;let out='',bytes=0;for(const ch of [...tail].reverse()){const n=encoder.encode(ch).length;if(bytes+n>2048)break;out=ch+out;bytes+=n;}
      row.assistant={...row.assistant,tail:out};
    }
    advanceTailActivity(row, 'responding');
    row.assistant={...row.assistant,content_offset:expected+deltaUnits,
      ...live,
      tail_offset: expected + deltaUnits,
      body:row.assistant.body?{...row.assistant.body,bytes:row.assistant.body.bytes+encoder.encode(delta).length}:undefined};
    e.slots.set(row.root_turn_id,materialize(row));notifyTail();return true;
  }
  return true;
}
