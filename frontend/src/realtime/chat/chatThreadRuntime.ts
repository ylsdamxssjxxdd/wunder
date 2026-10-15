// AI生成
// Durable chat pipeline hub: per-session thread state registry, server frame
// normalization and render invalidation. Chat has one protocol and one reducer.
import {
  applyChatThreadFrame,
  applyChatThreadSnapshot,
  applyChatThreadSnapshotPlanSharded,
  bindStreamStarted,
  planChatThreadSnapshot,
  type ChatThreadSnapshot,
  type ChatThreadSnapshotApplyOutcome
} from './chatThreadState';
import { planChatThreadSnapshotInWorker } from '@/workers/chatThreadSnapshotClient';
import { buildChatThreadTurnSlots } from './chatThreadProjection';
import {
  emptyChatThreadState,
  flattenThreadItemRow,
  hasEmbeddedItemPayload,
  THREAD_OVERFLOW_RESUME_COOLDOWN_MS,
  type ChatThreadFrame,
  type ThreadChangeFrame,
  type ChatThreadState,
  type ThreadTurnState,
  type StreamStartedAck,
  type ThreadSnapshotRequiredData
} from './chatThreadTypes';
import {
  markRuntimeProjectionChanged,
  markRuntimeProjectionContentChanged,
  markRuntimeProjectionReasoningChanged
} from './chatRuntimeProjectionInvalidation';
import type { ChatRuntimeMessageProjection, ChatRuntimeProjection } from './chatRuntimeTypes';
import {
  materializeChatRuntimeProjectionList
} from './chatRuntimeRenderAdapter';
import { chatPerf } from '@/utils/chatPerf';
import { threadViewSlots, threadViewCommitted, submitThreadView, clearThreadView, applyThreadViewChanges, applyThreadViewTail,threadViewStatus } from './threadViewStore';
import { threadViewStoreMessages } from '@/stores/chatGreeting';

interface ChatThreadRuntimeEntry {
  key: string;
  state: ChatThreadState;
  /** Caller-owned persistent projection shell: keeps the materialize row cache alive across bumps. */
  renderProjection: ChatRuntimeProjection;
  /** Clock (ms) of the last resume dispatch; collapses control-frame bursts. */
  lastResumeDispatchAt: number;
  /** W1-D: a snapshot_required reload is currently in flight for this session. */
  snapshotInFlight: boolean;
  /** W1-D: clock (ms) the last snapshot_required reload settled; a short
   *  cooldown collapses bursts that arrive after the reload completed. */
  lastSnapshotSettledAt: number;
  /** W2-F: cooperative abort for the in-flight sharded snapshot hydrate. */
  snapshotAbort: AbortController | null;
}

const registry = new Map<string, ChatThreadRuntimeEntry>();
const REGISTRY_LIMIT = 64;
/** Chat has no protocol selection: every web session uses durable changes. */
export const isChatChangeStreamEnabled = (): boolean => true;

export const ensureChatThreadRuntime = (key: string): ChatThreadRuntimeEntry => {
  let entry = registry.get(key);
  if (!entry) {
    entry = {
      key,
      state: emptyChatThreadState(key),
      renderProjection: { sessions: { [key]: { messages: [] } } } as unknown as ChatRuntimeProjection,
      lastResumeDispatchAt: 0,
      snapshotInFlight: false,
      lastSnapshotSettledAt: 0,
      snapshotAbort: null
    };
    registry.set(key, entry);
    if (registry.size > REGISTRY_LIMIT) {
      const oldest = registry.keys().next().value;
      if (oldest && oldest !== key) {
        // A sharded hydrate for an evicted session must stop; otherwise it
        // would keep writing into a state no longer owned by the registry.
        registry.get(oldest)?.snapshotAbort?.abort();
        registry.delete(oldest);
      }
    }
  }
  return entry;
};

export const getChatThreadState = (key: string): ChatThreadState | null =>
  registry.get(key)?.state ?? null;

export const isChatThreadV2Session = (key: string): boolean =>
  Boolean(registry.get(key)) && isChatChangeStreamEnabled();

export const resetChatThreadRuntime = (key: string): void => {
  // Abort any in-flight sharded hydrate before dropping the entry so it cannot
  // keep applying shards into a detached state.
  registry.get(key)?.snapshotAbort?.abort();
  registry.delete(key);
  clearThreadView(key);
};

export const getChatThreadStatus = (key: string): string | null => {
  const viewStatus=threadViewStatus(key);if(viewStatus!==null)return viewStatus;
  const state = getChatThreadState(key);
  if (!state?.turns.size) return null;
  let latest = null;
  let active = null;
  for (const turn of state.turns.values()) {
    if (!latest || (turn.userRound ?? 0) >= (latest.userRound ?? 0)) latest = turn;
    // `rejected` is a terminal admission result (for example USER_BUSY).
    // Treating it as active leaves the composer in a permanent busy state and
    // makes the next error appear to belong to the previous assistant bubble.
    if (turn.status && !['completed', 'failed', 'cancelled', 'interrupted', 'rejected', 'stopped'].includes(turn.status)) active = turn;
  }
  return active?.status ?? latest?.status ?? null;
};

/** Browser optimistic input belongs exclusively to the display window. */
export const submitChatThreadTurn = (_store:unknown,key:string,clientMessageId:string,content:string):void => {
  submitThreadView(key,clientMessageId,content);
};

/** Explicit raw-log projection consumers have their own optimistic input. */
export const submitRawChatThreadTurn=(store:unknown,key:string,clientMessageId:string,content:string):void=>{
  const state=ensureChatThreadRuntime(key).state;
  if([...state.turns.values()].some(turn=>turn.clientMessageId===clientMessageId))return;
  const userRound=[...state.turns.values()].reduce((max,turn)=>Math.max(max,turn.userRound??0),0)+1;
  const turnId=`pending:${clientMessageId}`;
  state.turns.set(turnId,{turnId,userRound,clientMessageId,userContent:content,status:'running'});
  markRuntimeProjectionChanged(store,{sessionId:key,reason:'thread_submit',immediate:true});
};

/**
 * Collect the locally-owned optimistic turns (`pending:` ids) that a snapshot
 * replacement must keep. Single pass over the turn map.
 */
const collectPendingTurns = (state: ChatThreadState): ThreadTurnState[] => {
  const pending: ThreadTurnState[] = [];
  for (const turn of state.turns.values()) {
    if (turn.turnId.startsWith('pending:')) pending.push(turn);
  }
  return pending;
};

/**
 * Re-attach the optimistic turns a snapshot did not already cover. One pass to
 * index the surviving client_message_ids, then one pass over the pending list:
 * O(turns + pending) instead of the former O(pending x turns) nested scan.
 */
const restorePendingTurns = (state: ChatThreadState, pending: ThreadTurnState[]): void => {
  if (!pending.length) return;
  const knownClientMessageIds = new Set<string | null>();
  for (const turn of state.turns.values()) knownClientMessageIds.add(turn.clientMessageId);
  for (const turn of pending) {
    if (knownClientMessageIds.has(turn.clientMessageId)) continue;
    state.turns.set(turn.turnId, turn);
    knownClientMessageIds.add(turn.clientMessageId);
  }
};

/** Synchronous compat hydrate (open-load path, dev fixtures, tests). */
export const hydrateChatThreadRuntime = (store: unknown, key: string, snapshot: ChatThreadSnapshot): boolean => {
  const state = ensureChatThreadRuntime(key).state;
  const pending = collectPendingTurns(state);
  const result = applyChatThreadSnapshot(state, snapshot, Date.now());
  if (!result.changed) return false;
  restorePendingTurns(state, pending);
  markRuntimeProjectionChanged(store, { sessionId: key, reason: 'thread_snapshot', immediate: true });
  return true;
};

export interface ChatThreadHydrateOptions {
  /** Cooperative abort for the sharded apply; checked before every shard. */
  signal?: AbortSignal;
  /** Main-thread yield between shards (default: a 0ms macrotask). */
  yieldToMain?: () => Promise<void>;
  /** Accumulated shard cost allowed before yielding (default per plan). */
  yieldBudget?: number;
}

/**
 * W2-F sharded hydrate: plan the snapshot (Worker when available, main thread
 * otherwise), then apply the shards with periodic main-thread yields and abort
 * support. Returns false when the snapshot is stale or the apply was aborted.
 */
export const hydrateChatThreadRuntimeBatched = async (
  store: unknown,
  key: string,
  snapshot: ChatThreadSnapshot,
  options: ChatThreadHydrateOptions = {}
): Promise<boolean> => {
  const state = ensureChatThreadRuntime(key).state;
  const pending = collectPendingTurns(state);
  const planned = await planChatThreadSnapshotInWorker(snapshot, undefined, options.signal);
  const plan = planned ?? planChatThreadSnapshot(snapshot);
  const outcome: ChatThreadSnapshotApplyOutcome = await applyChatThreadSnapshotPlanSharded(state, plan, {
    shouldAbort: () => options.signal?.aborted === true,
    yieldToMain: options.yieldToMain,
    yieldBudget: options.yieldBudget
  });
  if (!outcome.applied) return false;
  restorePendingTurns(state, pending);
  markRuntimeProjectionChanged(store, { sessionId: key, reason: 'thread_snapshot', immediate: true });
  return true;
};

/** Connection and user-interaction controls do not carry timeline state. */
const CONTROL_EVENT =
  /^(approval_|queue_|queued$|goal_|session_|command_session_|heartbeat$|ping$|thread_status$|thread_closed$|slow_client$|error$)/;

const asRowObject = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;

const readSeq = (...values: unknown[]): number | undefined => {
  for (const value of values) {
    const seq = Number(value);
    if (Number.isSafeInteger(seq) && seq >= 0) return seq;
  }
  return undefined;
};

/**
 * Map a server stream event to a protocol frame. Tolerated wire shapes
 * (transition period, plan §3.1):
 * - target contract: `{data: {change_type, turn_id, item_id, revision, cursor, payload}}`
 *   where `payload` is the immutable commit-time payload (for item_upsert a
 *   full item row with an embedded payload copy);
 * - feeder transition shape: `{data: {..., item: <full item row>}}`;
 * - emit-path frames: the change record nested once more under `data.data`.
 */
export const toChatThreadFrame = (
  eventType: string,
  payload: unknown
): ChatThreadFrame | StreamStartedAck | null => {
  const wire = payload as Record<string, unknown> | null | undefined;
  // Control frames may arrive with a sparse or missing payload; they must
  // still normalize so the caller-side recovery always fires.
  const envelope = (wire && typeof wire === 'object' && wire.data && typeof wire.data === 'object' && !Array.isArray(wire.data))
    ? wire.data as Record<string, unknown>
    : (wire && typeof wire === 'object' ? wire : {});
  const inner = asRowObject(envelope.data);

  if (eventType === 'thread_snapshot_required') {
    return {
      event: 'thread_snapshot_required',
      required_from_seq: readSeq(envelope.required_from_seq, inner?.required_from_seq),
      earliest_available_seq: readSeq(envelope.earliest_available_seq, inner?.earliest_available_seq)
    };
  }
  if (eventType === 'stream_overflow') {
    const cursor = readSeq(envelope.cursor, inner?.cursor);
    return {
      event: 'stream_overflow',
      session_id: typeof envelope.session_id === 'string' && envelope.session_id
        ? envelope.session_id
        : typeof inner?.session_id === 'string' && inner.session_id ? inner.session_id : undefined,
      cursor,
      resume_recommended: (envelope.resume_recommended ?? inner?.resume_recommended) === true
    };
  }
  if (eventType === 'thread_item_tail') {
    const item_id = String(envelope.item_id ?? inner?.item_id ?? '');
    if (!item_id) return null;
    return {
      event: 'thread_item_tail',
      item_id,
      field: String(envelope.field ?? inner?.field ?? 'content'),
      offset: Number(envelope.offset ?? inner?.offset ?? -1),
      base_seq: readSeq(envelope.base_seq, inner?.base_seq),
      text: String(envelope.text ?? inner?.text ?? '')
    };
  }
  if (eventType === 'stream_started') {
    const source = inner ?? envelope;
    const turn_id = String(source.turn_id ?? '');
    if (!turn_id) return null;
    // The ack only binds client_message_id → turn_id; bubble content never
    // travels through it (plan §3.2).
    return {
      event: 'stream_started',
      turn_id,
      user_round: Number(source.user_round) > 0 ? Number(source.user_round) : undefined,
      resume_from_seq: readSeq(source.resume_from_seq, source.change_cursor) ?? 0,
      client_message_id: source.client_message_id ? String(source.client_message_id) : undefined
    };
  }
  if (eventType === 'thread_item_block') {
    const seq = Number(envelope.cursor ?? inner?.cursor ?? 0);
    const item_id = String(envelope.item_id ?? inner?.item_id ?? '');
    if (!Number.isSafeInteger(seq) || seq <= 0 || !item_id) return null;
    return {
      event: 'thread_change',
      seq,
      change_type: 'text_block',
      item_id,
      turn_id: String(envelope.turn_id ?? ''),
      revision: 0,
      data: {
        item_id,
        field: String(envelope.field ?? inner?.field ?? 'content'),
        block_index: Number(envelope.block_index ?? inner?.block_index ?? 0),
        content: envelope.content ?? inner?.content,
        content_offset: envelope.content_offset ?? inner?.content_offset,
        reasoning: envelope.reasoning ?? inner?.reasoning,
        reasoning_offset: envelope.reasoning_offset ?? inner?.reasoning_offset
      }
    };
  }
  if (eventType === 'thread_change') {
    const seq = Number(envelope.cursor ?? inner?.cursor ?? 0);
    if (!Number.isSafeInteger(seq) || seq <= 0) return null;
    const change_type = String(envelope.change_type ?? inner?.change_type ?? '');
    const itemRow = asRowObject(envelope.item) ?? asRowObject(inner?.item);
    const rowPayload = asRowObject(envelope.payload) ?? asRowObject(inner?.payload);
    let data: Record<string, unknown> = {};
    if (itemRow) {
      data = flattenThreadItemRow(itemRow);
    } else if (rowPayload) {
      // Target contract: the item_upsert payload is the full immutable item
      // row (columns + embedded payload copy). Flat payloads and the
      // turn/block payloads pass through untouched.
      data = change_type === 'item_upsert' && hasEmbeddedItemPayload(rowPayload)
        ? flattenThreadItemRow(rowPayload)
        : rowPayload;
    }
    return {
      event: 'thread_change',
      seq,
      change_type,
      item_id: typeof envelope.item_id === 'string' && envelope.item_id
        ? envelope.item_id
        : typeof inner?.item_id === 'string' && inner.item_id ? inner.item_id : undefined,
      turn_id: typeof envelope.turn_id === 'string' && envelope.turn_id
        ? envelope.turn_id
        : typeof inner?.turn_id === 'string' && inner.turn_id ? inner.turn_id : undefined,
      revision: Number(envelope.revision ?? inner?.revision ?? 0) || undefined,
      data
    };
  }
  return null;
};

const bumpInvalidation = (
  store: unknown,
  key: string,
  frame: ChatThreadFrame,
  changed: boolean,
  structural: boolean
): void => {
  if (!changed) return;
  if (frame.event === 'thread_item_tail') {
    // Each user turn owns one assistant bubble. A tail therefore invalidates
    // its turn-level row rather than a transient model-round row.
    const state = getChatThreadState(key);
    const item = state?.items.get(frame.item_id);
    const executionId = item?.turnId || String(frame.item_id || '').split(':', 1)[0];
    const turnId = String(item?.raw.root_turn_id || state?.turns.get(executionId)?.rootTurnId || executionId);
    const messageId = turnId ? `tturn:${turnId}:assistant` : `titem:${frame.item_id}`;
    if (frame.field === 'reasoning') {
      markRuntimeProjectionReasoningChanged(store, [messageId]);
    } else {
      markRuntimeProjectionContentChanged(store, [messageId]);
    }
    return;
  }
  if (structural) {
    markRuntimeProjectionChanged(store, { sessionId: key, reason: 'thread_structure' });
    return;
  }
  markRuntimeProjectionChanged(store, { sessionId: key, reason: 'thread_revision' });
};

/**
 * Caller hooks for applyChatThreadServerEvent (plan §5 M2-A/M2-C).
 * - onSnapshotApplied: the runtime rebuilt the state from the atomic snapshot
 *   (lastSeq = snapshot cursor); the caller should re-watch from lastSeq.
 * - onSnapshotRequired: the runtime could NOT rebuild (no loader registered,
 *   load failed or snapshot stale); the caller must stop and report recovery failure.
 * - onOverflow / onGapOverflow: the caller resumes the watch from lastSeq
 *   (abort + start; no full reload). Dispatches share one per-session cooldown
 *   (THREAD_OVERFLOW_RESUME_COOLDOWN_MS) so bursts collapse into one resume.
 * - now: injectable clock for tests; defaults to Date.now.
 */
export interface ChatThreadServerEventHooks {
  onViewApplied?: (prior:Map<string,string>) => void;
  now?: () => number;
  onChangesApplied?: (changes: ThreadChangeFrame[]) => void;
  onSnapshotRequired?: (payload: ThreadSnapshotRequiredData) => void;
  onSnapshotApplied?: () => void;
  onOverflow?: (cursor: number) => void;
  onGapOverflow?: () => void;
}

/**
 * Atomic snapshot loader for snapshot_required recovery (plan §5 M2-C). The
 * runtime module stays API-free: the app registers the real implementation
 * (chatWatcher), tests register fakes. null clears the registration.
 */
export type ChatThreadSnapshotLoader = (sessionKey: string) => Promise<ChatThreadSnapshot>;
let chatThreadSnapshotLoader: ChatThreadSnapshotLoader | null = null;
export const registerChatThreadSnapshotLoader = (loader: ChatThreadSnapshotLoader | null): void => {
  chatThreadSnapshotLoader = loader;
};

const extractSnapshotRequiredData = (payload: unknown): ThreadSnapshotRequiredData => {
  const frame = toChatThreadFrame('thread_snapshot_required', payload);
  if (frame && frame.event === 'thread_snapshot_required') {
    return {
      required_from_seq: frame.required_from_seq,
      earliest_available_seq: frame.earliest_available_seq
    };
  }
  return {};
};

/** W1-D: collapse snapshot_required bursts. At most one reload may be in
 *  flight per session; after it settles a short cooldown swallows follow-up
 *  frames, so a flapping watch cannot degrade into a reload storm. */
const THREAD_SNAPSHOT_REQUIRED_COOLDOWN_MS = 1_000;

const dispatchSnapshotRequired = (
  store: unknown,
  key: string,
  payload: unknown,
  hooks: ChatThreadServerEventHooks
): void => {
  const data = extractSnapshotRequiredData(payload);
  const entry = ensureChatThreadRuntime(key);
  const loader = chatThreadSnapshotLoader;
  const now = (hooks.now ?? Date.now)();
  // Single-flight: a reload is already running for this session.
  if (entry.snapshotInFlight) return;
  // Cooldown: a reload just settled; drop the trailing burst.
  if (entry.lastSnapshotSettledAt && now - entry.lastSnapshotSettledAt < THREAD_SNAPSHOT_REQUIRED_COOLDOWN_MS) {
    return;
  }
  if (!loader) {
    entry.lastSnapshotSettledAt = now;
    hooks.onSnapshotRequired?.(data);
    return;
  }
  entry.snapshotInFlight = true;
  // W2-F: the sharded hydrate is abortable. The controller is aborted when the
  // session is reset or evicted, so a superseded rebuild stops between shards.
  const controller = new AbortController();
  entry.snapshotAbort = controller;
  void (async () => {
    try {
      const snapshot = await loader(key);
      const current = registry.get(key);
      if (!current) return;
      const applied = await hydrateChatThreadRuntimeBatched(store, key, snapshot, { signal: controller.signal });
      if (!applied) throw new Error('thread snapshot cursor is stale');
      markRuntimeProjectionChanged(store, { sessionId: key, reason: 'thread_structure' });
      hooks.onSnapshotApplied?.();
    } catch {
      // W4-L: an aborted or failed hydrate is the heavy-history abort signal.
      // A deliberate abort (session reset/eviction) is not a recovery failure.
      chatPerf.countSnapshotAbort(1);
      if (!controller.signal.aborted) hooks.onSnapshotRequired?.(data);
    } finally {
      entry.snapshotInFlight = false;
      entry.lastSnapshotSettledAt = (hooks.now ?? Date.now)();
      if (entry.snapshotAbort === controller) entry.snapshotAbort = null;
    }
  })();
};

const dispatchResume = (
  entry: ChatThreadRuntimeEntry,
  hooks: ChatThreadServerEventHooks,
  cursor: number,
  reason: 'gap' | 'overflow'
): void => {
  const now = (hooks.now ?? Date.now)();
  // Suppress duplicate resumes: the first control frame starts the recovery,
  // followers inside the cooldown window are dropped.
  if (entry.lastResumeDispatchAt && now - entry.lastResumeDispatchAt < THREAD_OVERFLOW_RESUME_COOLDOWN_MS) {
    return;
  }
  entry.lastResumeDispatchAt = now;
  if (reason === 'overflow') hooks.onOverflow?.(cursor);
  else hooks.onGapOverflow?.();
};

/**
 * Apply one server event to the v2 thread state. Returns true when the event
 * was consumed by this pipeline (caller must skip duplicate application).
 */
export const applyChatThreadServerEvent = (
  store: unknown,
  key: string,
  eventType: string,
  payload: unknown,
  hooks: ChatThreadServerEventHooks = {}
): boolean => {
  // This is an out-of-band filesystem notification, not timeline data. Let
  // every transport deliver its side effect even after a view has committed.
  if (eventType === 'workspace_update') return false;
  // Queue controls are handled before the durable pipeline classifies them as
  // control frames. This makes queue_start immediately consume the composer
  // item instead of waiting for an eventually consistent queue refresh.
  const normalizedThreadChange = eventType === 'thread_change'
    ? toChatThreadFrame(eventType, payload)
    : null;
  // The display window only owns the live tail once it has committed a page.
  // While an open is still in flight (or failed) `applyThreadViewTail` finds no
  // row and would swallow the delta, so fall through to the durable state.
  if(eventType==='thread_item_tail' && threadViewCommitted(key) && applyThreadViewTail(key,payload))return true;
  if (eventType === 'thread_view_change') {
    const wire = payload as any;
    const data = wire?.data ?? wire;
    const prior=new Map((threadViewSlots(key)??[]).map(s=>[s.rootTurnId,String(s.assistant.status)]));
    const applied = applyThreadViewChanges(key, data);
    if (!applied) hooks.onSnapshotRequired?.({});
    else {
      const target=store as any;
      if(target.activeSessionId===key)target.messages=threadViewStoreMessages(target,key);
      const active=['running','queued','waiting_input','waiting_approval'].includes(threadViewStatus(key)??'');
      if(target.loadingBySession) {if(active)target.loadingBySession[key]=true;else delete target.loadingBySession[key];}
      hooks.onViewApplied?.(prior);
    }
    return true;
  }
  if(eventType==='thread_view_required'){hooks.onSnapshotRequired?.({});return true;}
  if (CONTROL_EVENT.test(eventType)||eventType==='final') return false;
  // An uncommitted window entry must not shadow the durable transcript either:
  // the frames below are the only producer of it until the page commits.
  if(threadViewCommitted(key))return true;
  const entry = ensureChatThreadRuntime(key);
  if (eventType === 'thread_snapshot_required') {
    dispatchSnapshotRequired(store, key, payload, hooks);
    return true;
  }
  const frame = normalizedThreadChange ?? toChatThreadFrame(eventType, payload);
  if (!frame) return true;
  if (frame.event === 'stream_started') {
    bindStreamStarted(entry.state, frame);
    markRuntimeProjectionChanged(store, { sessionId: key, reason: 'thread_turn_binding', immediate: true });
    return true;
  }
  if (frame.event === 'stream_overflow') {
    dispatchResume(entry, hooks, typeof frame.cursor === 'number' ? frame.cursor : 0, 'overflow');
    return true;
  }
  const changes: ThreadChangeFrame[] = [];
  const result = applyChatThreadFrame(entry.state, frame, (hooks.now ?? Date.now)(), (change) => changes.push(change));
  if (changes.length) hooks.onChangesApplied?.(changes);
  if (result.needResume) {
    dispatchResume(entry, hooks, 0, 'gap');
    return true;
  }
  bumpInvalidation(store, key, frame, result.changed, result.structural);
  return true;
};

/**
 * Deterministic durable render source: thread state -> projection bubbles ->
 * materialized rows.
 */
export type ChatThreadMaterializedSlot = {
  key: string;
  rootTurnId: string;
  user: Record<string, any>;
  assistant: Record<string, any>;
};

export const buildChatThreadMaterializedSlots = (key: string): ChatThreadMaterializedSlot[] | null => {
  const view = threadViewSlots(key);
  // Prefer the committed window, but keep rendering from the durable state
  // while an open is still in flight so the transcript never blanks out.
  if (view !== null && threadViewCommitted(key)) return view;
  const entry = registry.get(key);
  if (!entry) return null;
  const slots = buildChatThreadTurnSlots(entry.state);
  const messages = slots.flatMap(slot => [slot.user, slot.assistant]);
  const session = (entry.renderProjection.sessions as unknown as Record<string, { messages: ChatRuntimeMessageProjection[] }>)[key];
  if (session) session.messages = messages;
  const materialized = materializeChatRuntimeProjectionList(
    entry.renderProjection, key, messages, { trustProjectionVersions: true }
  );
  return slots.map((slot, index) => ({ key: slot.key, rootTurnId: slot.rootTurnId,
    user: materialized[index * 2], assistant: materialized[index * 2 + 1] }));
};

export const buildChatThreadMaterializedMessages = (key: string): ChatRuntimeMessageProjection[] | null => {
  const slots = buildChatThreadMaterializedSlots(key);
  return slots?.flatMap(slot => [slot.user, slot.assistant]) as ChatRuntimeMessageProjection[] ?? null;
};
