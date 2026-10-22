// Chat streaming pipeline v2 — client state layer (pure reducer).
// Spec: docs/聊天流式管线根治方案.md §3.3 (frame protocol), §3.4 (state machine
// rules), §5.5 (bounded buffers). Pure TypeScript: no Vue/Pinia imports; every
// mutation happens in place on the ChatThreadState maps and the caller owns
// reactive invalidation.
//
// Side store: ChatThreadState is a frozen cross-module contract
// (chatThreadTypes.ts) with no slot for two reducer internals, so they live
// in a WeakMap keyed by the state instance (per-session, GC'd with the state,
// reset by applyChatThreadSnapshot):
//   1. the monotonic ThreadItemState.order counter — items.size cannot derive
//      it because the items map shrinks on snapshot replacement and would
//      repeat values;
//   2. tails for fields other than content/reasoning (tool/command output),
//      which ThreadTailState cannot represent.
//
// ChatThreadApplyResult.droppedEphemeral counts ephemeral tail frames the
// reducer discarded (offset mismatch against the current tail position).

import { flattenThreadItemRow, hasEmbeddedItemPayload } from './chatThreadTypes';
import type {
  ChatThreadApplyResult,
  ChatThreadField,
  ChatThreadFrame,
  ChatThreadState,
  StreamStartedAck,
  ThreadChangeFrame,
  ThreadItemState,
  ThreadItemTailFrame,
  ThreadItemUpsertPayload,
  ThreadTailState,
  ThreadTurnState,
  ThreadTurnStatusPayload,
  ThreadTurnUpsertPayload
} from './chatThreadTypes';
import { chatPerf } from '@/utils/chatPerf';

// W4-L: guarded clock for the heavy-history metrics. `performance` is missing in
// some non-browser hosts, and every chatPerf recorder is inert while a capture
// is not running, so leaving the calls in place costs two clock reads.
const perfNowMs = (): number =>
  typeof performance !== 'undefined' ? performance.now() : Date.now();

export interface ChatThreadSnapshotTurn {
  root_turn_id?: string;
  turn_id: string;
  user_round?: number;
  status?: string;
  content?: string;
  client_message_id?: string;
  user_turn_index?: number;
  payload?: Record<string, unknown>;
}

/**
 * Full reload payload (first load, thread_snapshot_required, explicit refresh).
 * Snapshot items may arrive either as flat projections (legacy) or as full
 * immutable thread_items rows with an embedded payload copy (atomic snapshot
 * API, plan §5 M2-C); both shapes are normalized below.
 */
export interface ChatThreadSnapshot {
  cursor: number;
  turns: ChatThreadSnapshotTurn[];
  items: ThreadItemUpsertPayload[];
}

// Shared verdict objects: the reducer never mutates results, so hot paths can
// return these without allocating.
const UNCHANGED: ChatThreadApplyResult = { changed: false, structural: false, droppedEphemeral: 0 };
const CHANGED_CONTENT: ChatThreadApplyResult = { changed: true, structural: false, droppedEphemeral: 0 };
const CHANGED_STRUCTURAL: ChatThreadApplyResult = { changed: true, structural: true, droppedEphemeral: 0 };
const DROPPED_TAIL: ChatThreadApplyResult = { changed: false, structural: false, droppedEphemeral: 1 };

interface FieldTail {
  offset: number;
  text: string;
}

interface ChatThreadSideState {
  orderCounter: number;
  /** Tails for fields outside content/reasoning (tool/command output). */
  extraTails: Map<string, FieldTail>;
}

const isCancelledTurnStatus = (value: unknown): boolean => {
  const normalized = String(value ?? '').trim().toLowerCase();
  return normalized === 'cancelled' || normalized === 'canceled' || normalized === 'aborted';
};

const sideStates = new WeakMap<ChatThreadState, ChatThreadSideState>();

function sideState(state: ChatThreadState): ChatThreadSideState {
  let side = sideStates.get(state);
  if (!side) {
    side = { orderCounter: 0, extraTails: new Map() };
    sideStates.set(state, side);
  }
  return side;
}

function tailCoordinate(itemId: string, field: ChatThreadField): string {
  return `${itemId}\u0000${field}`;
}

function firstString(...values: unknown[]): string | undefined {
  for (const value of values) {
    if (typeof value === 'string' && value) return value;
  }
  return undefined;
}

function firstNumber(...values: unknown[]): number | undefined {
  for (const value of values) {
    if (typeof value === 'number' && Number.isFinite(value)) return value;
  }
  return undefined;
}

function nextItemOrder(state: ChatThreadState): number {
  const side = sideState(state);
  const order = side.orderCounter;
  side.orderCounter += 1;
  return order;
}

// ---------------------------------------------------------------------------
// Frame entry points
// ---------------------------------------------------------------------------

export function applyChatThreadFrame(
  state: ChatThreadState,
  frame: ChatThreadFrame,
  now: number,
  onChange?: (frame: ThreadChangeFrame) => void
): ChatThreadApplyResult {
  switch (frame.event) {
    case 'thread_change':
      return applyChangeFrame(state, frame, now, onChange);
    case 'thread_item_tail':
      return applyTailFrame(state, frame);
    case 'thread_snapshot_required':
    case 'stream_overflow':
      // The state layer stays inert; the caller owns the full reload / resume.
      return UNCHANGED;
    default:
      return UNCHANGED;
  }
}

function applyChangeFrame(
  state: ChatThreadState,
  frame: ThreadChangeFrame,
  now: number,
  onChange?: (frame: ThreadChangeFrame) => void
): ChatThreadApplyResult {
  const seq = frame.seq;
  if (typeof seq !== 'number' || !Number.isFinite(seq)) return UNCHANGED;
  if (seq <= state.lastSeq) return UNCHANGED; // idempotent replay
  // Model-context records do not produce display frames, so valid rollout
  // cursors are ordered but intentionally sparse. Transport order is stable.
  void now;
  const result = applyInOrderChange(state, frame);
  state.lastSeq = seq;
  if (result.changed) onChange?.(frame);
  return result;
}

function applyInOrderChange(state: ChatThreadState, frame: ThreadChangeFrame): ChatThreadApplyResult {
  const data = frame.data;
  const current = typeof data.item_id === 'string' ? state.items.get(data.item_id) : undefined;
  if (typeof data.client_message_id === 'string' && data.client_message_id &&
      (frame.change_type === 'turn_upsert' ||
       (frame.change_type === 'item_upsert' && data.kind === 'user_message' &&
        (!current || Number(data.revision ?? frame.revision ?? 0) > current.revision)))) {
    bindStreamStarted(state, {
      event: 'stream_started', turn_id: String(data.turn_id ?? frame.turn_id ?? ''),
      client_message_id: data.client_message_id, resume_from_seq: frame.seq,
      user_round: typeof data.user_round === 'number' ? data.user_round : undefined
    });
  }
  switch (frame.change_type) {
    case 'item_upsert':
      return applyItemUpsert(state, frame);
    case 'turn_upsert':
      return applyTurnUpsert(state, frame);
    case 'turn_status':
      return applyTurnStatus(state, frame);
    default:
      // Forward compatibility: unknown durable change types are consumed
      // (they advance lastSeq) but change nothing.
      return UNCHANGED;
  }
}

// ---------------------------------------------------------------------------
// Durable change appliers
// ---------------------------------------------------------------------------

function applyItemUpsert(state: ChatThreadState, frame: ThreadChangeFrame): ChatThreadApplyResult {
  const data = frame.data as ThreadItemUpsertPayload;
  const itemId = firstString(data.item_id, frame.item_id);
  if (!itemId) return UNCHANGED;
  const revision = firstNumber(data.revision, frame.revision) ?? 0;
  const existing = state.items.get(itemId);

  if (!existing) {
    const item: ThreadItemState = {
      itemId,
      turnId: firstString(data.turn_id, frame.turn_id) ?? '',
      modelRound: firstNumber(data.model_round) ?? 0,
      kind: firstString(data.kind) ?? '',
      status: firstString(data.status) ?? '',
      revision,
      role: firstString(data.role) ?? null,
      visibility: firstString(data.visibility) ?? null,
      content: typeof data.content === 'string' ? data.content : '',
      reasoning: typeof data.reasoning === 'string' ? data.reasoning :
        typeof data.reasoning_content === 'string' ? data.reasoning_content : '',
      order: nextItemOrder(state),
      raw: data
    };
    state.items.set(itemId, item);
    settleItemTails(state, item);
    if (item.kind === 'user_message') syncUserTurn(state, item);
    // A terminal item carries the turn outcome; admin-visibility items are
    // stored normally here, exclusion is the projection's job.
    if (item.kind === 'terminal' && item.status) syncTerminalTurnStatus(state, item);
    return CHANGED_STRUCTURAL;
  }

  // Same revision is idempotent; a lower revision is stale.
  if (revision <= existing.revision) return UNCHANGED;
  // A delayed model-final upsert may still carry authoritative terminal usage
  // statistics. Accept its revision and raw metadata, but do not replace the
  // partial text a user explicitly stopped. Tool items settle independently.
  const preserveStoppedText = existing.kind === 'assistant_message' &&
    isCancelledTurnStatus(state.turns.get(existing.turnId)?.status);
  // Child progress is card metadata, never a token tail. A revision must
  // invalidate the parent row even while its own turn is already terminal.
  let structural = existing.kind === 'subagent_run';
  existing.revision = revision;
  existing.raw = data;
  const modelRound = firstNumber(data.model_round);
  if (modelRound !== undefined && modelRound !== existing.modelRound) {
    existing.modelRound = modelRound;
    structural = true;
  }
  const status = firstString(data.status);
  if (status !== undefined && status !== existing.status) {
    existing.status = status;
    structural = true;
  }
  if (!preserveStoppedText && typeof data.content === 'string') existing.content = data.content;
  const reasoning = typeof data.reasoning === 'string' ? data.reasoning :
    typeof data.reasoning_content === 'string' ? data.reasoning_content : undefined;
  if (!preserveStoppedText && reasoning !== undefined) existing.reasoning = reasoning;
  settleItemTails(state, existing);
  const role = firstString(data.role);
  if (role !== undefined) existing.role = role;
  const visibility = firstString(data.visibility);
  if (visibility !== undefined) existing.visibility = visibility;
  if (existing.kind === 'user_message') syncUserTurn(state, existing);
  if (existing.kind === 'terminal' && status !== undefined) syncTerminalTurnStatus(state, existing);
  return structural ? CHANGED_STRUCTURAL : CHANGED_CONTENT;
}

function syncUserTurn(state: ChatThreadState, item: ThreadItemState): void {
  if (item.visibility === 'admin' || item.visibility === 'model_internal') return;
  const turn = state.turns.get(item.turnId) ?? {
    turnId: item.turnId, userRound: null, status: null, clientMessageId: null, userContent: null
  };
  turn.userContent = item.content;
  turn.userRound = firstNumber(item.raw.user_round) ?? turn.userRound;
  turn.clientMessageId = firstString(item.raw.client_message_id) ?? turn.clientMessageId;
  state.turns.set(item.turnId, turn);
}

function syncTerminalTurnStatus(state: ChatThreadState, item: ThreadItemState): void {
  const turn = state.turns.get(item.turnId);
  if (!turn) {
    state.turns.set(item.turnId, {
      turnId: item.turnId,
      userRound: null,
      status: item.status,
      clientMessageId: null,
      userContent: null
    });
    return;
  }
  turn.status = item.status;
}

function applyTurnUpsert(state: ChatThreadState, frame: ThreadChangeFrame): ChatThreadApplyResult {
  const data = frame.data as unknown as ThreadTurnUpsertPayload;
  const turnId = firstString(data.turn_id, frame.turn_id);
  if (!turnId) return UNCHANGED;
  const existing = state.turns.get(turnId);

  if (!existing) {
    state.turns.set(turnId, {
      turnId,
      stopReason: firstString(data.stop_reason),
      rootTurnId: firstString(data.root_turn_id),
      userRound: firstNumber(data.user_round) ?? null,
      status: firstString(data.status) ?? null,
      clientMessageId: firstString(data.client_message_id) ?? null,
      userContent: typeof data.content === 'string' ? data.content : null
    });
    return CHANGED_STRUCTURAL;
  }

  let changed = false;
  let structural = false;
  const stopReason = firstString(data.stop_reason);
  if (stopReason !== undefined && stopReason !== existing.stopReason) {
    existing.stopReason = stopReason;
    changed = true;
  }
  const rootTurnId = firstString(data.root_turn_id);
  if (rootTurnId && rootTurnId !== existing.rootTurnId) {
    existing.rootTurnId = rootTurnId;
    changed = true;
    structural = true;
  }
  const userRound = firstNumber(data.user_round);
  if (userRound !== undefined && userRound !== existing.userRound) {
    existing.userRound = userRound;
    changed = true;
    structural = true;
  }
  const status = firstString(data.status);
  if (status !== undefined && status !== existing.status) {
    existing.status = status;
    changed = true;
    structural = true;
  }
  const clientMessageId = firstString(data.client_message_id);
  if (clientMessageId !== undefined && clientMessageId !== existing.clientMessageId) {
    existing.clientMessageId = clientMessageId;
    changed = true;
  }
  if (typeof data.content === 'string' && data.content !== existing.userContent) {
    existing.userContent = data.content;
    changed = true;
  }
  return changed ? (structural ? CHANGED_STRUCTURAL : CHANGED_CONTENT) : UNCHANGED;
}

function applyTurnStatus(state: ChatThreadState, frame: ThreadChangeFrame): ChatThreadApplyResult {
  const data = frame.data as unknown as ThreadTurnStatusPayload;
  const turnId = firstString(data.turn_id, frame.turn_id);
  const status = firstString(data.status);
  if (!turnId || status === undefined) return UNCHANGED;
  const existing = state.turns.get(turnId);
  if (!existing) {
    // Defensive: keep the transition even if turn_upsert has not been seen.
    state.turns.set(turnId, {
      turnId,
      userRound: null,
      stopReason: firstString(data.stop_reason),
      status,
      clientMessageId: null,
      userContent: null
    });
    return CHANGED_STRUCTURAL;
  }
  const stopReason = firstString(data.stop_reason);
  if (existing.status === status && (stopReason === undefined || stopReason === existing.stopReason)) return UNCHANGED;
  if (stopReason !== undefined) existing.stopReason = stopReason;
  existing.status = status;
  return CHANGED_STRUCTURAL;
}

// ---------------------------------------------------------------------------
// Ephemeral tail (thread_item_tail)
// ---------------------------------------------------------------------------

function readFieldTail(state: ChatThreadState, itemId: string, field: ChatThreadField): FieldTail | null {
  if (field === 'content' || field === 'reasoning') {
    const tail = state.tails.get(itemId);
    if (!tail) return null;
    return field === 'content'
      ? { offset: tail.contentOffset, text: tail.content }
      : { offset: tail.reasoningOffset, text: tail.reasoning };
  }
  const stored = sideState(state).extraTails.get(tailCoordinate(itemId, field));
  return stored ? { offset: stored.offset, text: stored.text } : null;
}

function writeFieldTail(
  state: ChatThreadState,
  itemId: string,
  field: ChatThreadField,
  offset: number,
  text: string
): void {
  if (field === 'content' || field === 'reasoning') {
    let tail = state.tails.get(itemId);
    if (!tail) {
      tail = { contentOffset: 0, reasoningOffset: 0, content: '', reasoning: '' };
      state.tails.set(itemId, tail);
    }
    if (field === 'content') {
      tail.contentOffset = offset;
      tail.content = text;
    } else {
      tail.reasoningOffset = offset;
      tail.reasoning = text;
    }
    return;
  }
  sideState(state).extraTails.set(tailCoordinate(itemId, field), { offset, text });
}

function truncateFieldTail(state: ChatThreadState, itemId: string, field: ChatThreadField, completedEnd: number): void {
  if (field === 'content' || field === 'reasoning') {
    const tail = state.tails.get(itemId);
    if (!tail) return;
    if (field === 'content') {
      if (tail.contentOffset < completedEnd) {
        tail.content = tail.content.slice(completedEnd - tail.contentOffset);
        tail.contentOffset = completedEnd;
      }
      return;
    }
    if (tail.reasoningOffset < completedEnd) {
      tail.reasoning = tail.reasoning.slice(completedEnd - tail.reasoningOffset);
      tail.reasoningOffset = completedEnd;
    }
    return;
  }
  const stored = sideState(state).extraTails.get(tailCoordinate(itemId, field));
  if (stored && stored.offset < completedEnd) {
    stored.text = stored.text.slice(completedEnd - stored.offset);
    stored.offset = completedEnd;
  }
}

function itemFieldText(item: ThreadItemState | undefined, field: ChatThreadField): string {
  if (!item) return '';
  if (field === 'content') return item.content;
  if (field === 'reasoning') return item.reasoning;
  const value = item.raw[field];
  return typeof value === 'string' ? value : '';
}

function settleItemTails(state: ChatThreadState, item: ThreadItemState): void {
  truncateFieldTail(state, item.itemId, 'content', item.content.length);
  truncateFieldTail(state, item.itemId, 'reasoning', item.reasoning.length);
  const prefix = `${item.itemId}\u0000`;
  for (const key of sideState(state).extraTails.keys()) {
    if (!key.startsWith(prefix)) continue;
    const field = key.slice(prefix.length);
    truncateFieldTail(state, item.itemId, field, itemFieldText(item, field).length);
  }
}

function applyTailFrame(state: ChatThreadState, frame: ThreadItemTailFrame): ChatThreadApplyResult {
  const itemId = firstString(frame.item_id);
  if (!itemId) return UNCHANGED;
  const field = firstString(frame.field) ?? 'content';
  const text = typeof frame.text === 'string' ? frame.text : '';
  if (!text) return UNCHANGED;
  // Plan §2 I3: the writer only emits a tail after its base durable frame, and
  // the client only applies it once the replay reached that frame. A tail
  // ahead of lastSeq belongs to a frame we have not applied yet — drop it and
  // let the completed item replace the missing text. Missing base_seq is
  // treated as 0 for legacy senders.
  const baseSeq = typeof frame.base_seq === 'number' && Number.isFinite(frame.base_seq)
    ? frame.base_seq
    : 0;
  if (state.lastSeq < baseSeq) return DROPPED_TAIL;

  const current = readFieldTail(state, itemId, field);
  // An empty stored tail (fresh field slot or covered by a completed item) restarts
  // from the completed item prefix.
  const item = state.items.get(itemId);
  const completed = itemFieldText(item, field);
  const position = current && current.text
    ? current.offset + current.text.length
    : completed.length;
  if (frame.offset !== -1 && frame.offset !== position) {
    // offset < position: already covered by a completed item or an earlier
    // append; offset > position: hole — wait for the next completed item.
    return DROPPED_TAIL;
  }
  const baseOffset = current && current.text ? current.offset : position;
  writeFieldTail(state, itemId, field, baseOffset, (current ? current.text : '') + text);
  return CHANGED_CONTENT;
}

// ---------------------------------------------------------------------------
// Snapshot (full reload with cursor guard, section 3.4 rule 4)
//
// W2-F: the reload payload is first normalized into a bounded, serializable
// shard plan (planChatThreadSnapshot) and then applied shard by shard. The
// planner is pure and does row flattening only, so it can run in a Worker for
// large threads; the applier yields to the main thread once a shard budget is
// spent and honours an abort signal, so any single main-thread task stays
// bounded (decision D5: <= 200ms per task). Shard order is turn phase, then
// item phase, so the applied state is identical to the
// pre-sharding single-pass apply.
// ---------------------------------------------------------------------------

export interface ChatThreadSnapshotShard {
  kind: 'turns' | 'items';
  turns?: ChatThreadSnapshotTurn[];
  items?: ThreadItemUpsertPayload[];
  /** Work units (turn/item count) used for main-thread yield budgeting. */
  cost: number;
}

export interface ChatThreadSnapshotPlan {
  cursor: number;
  shards: ChatThreadSnapshotShard[];
  turnCount: number;
  itemCount: number;
}

export interface ChatThreadSnapshotPlanOptions {
  maxTurnsPerShard?: number;
  maxItemsPerShard?: number;
}

export const CHAT_THREAD_SNAPSHOT_PLAN_DEFAULTS = {
  maxTurnsPerShard: 128,
  maxItemsPerShard: 512
} as const;

/** Accumulated shard cost allowed between two main-thread yields. */
export const CHAT_THREAD_SNAPSHOT_YIELD_BUDGET = 512;

function chunkArray<T>(values: T[], size: number): T[][] {
  const step = Math.max(1, Math.floor(size));
  const chunks: T[][] = [];
  for (let index = 0; index < values.length; index += step) {
    chunks.push(values.slice(index, index + step));
  }
  return chunks;
}

/**
 * Normalize a full/partial reload into a bounded shard plan. Embedded item rows
 * (atomic snapshot shape) are flattened here: this is the CPU-heavy parse the
 * Worker offloads. Shards are contiguous slices in phase order.
 */
export function planChatThreadSnapshot(
  snapshot: ChatThreadSnapshot | null | undefined,
  options: ChatThreadSnapshotPlanOptions = {}
): ChatThreadSnapshotPlan {
  const cursor = snapshot && typeof snapshot.cursor === 'number' && Number.isFinite(snapshot.cursor)
    ? snapshot.cursor
    : Number.NaN;
  const turns = Array.isArray(snapshot?.turns) ? snapshot.turns : [];
  const sourceItems = Array.isArray(snapshot?.items) ? snapshot.items : [];
  const items = sourceItems.map((entry) => hasEmbeddedItemPayload(entry as Record<string, unknown>)
    ? flattenThreadItemRow(entry as Record<string, unknown>) as unknown as ThreadItemUpsertPayload
    : entry);
  const maxTurns = options.maxTurnsPerShard ?? CHAT_THREAD_SNAPSHOT_PLAN_DEFAULTS.maxTurnsPerShard;
  const maxItems = options.maxItemsPerShard ?? CHAT_THREAD_SNAPSHOT_PLAN_DEFAULTS.maxItemsPerShard;
  const shards: ChatThreadSnapshotShard[] = [];
  for (const chunk of chunkArray(turns, maxTurns)) shards.push({ kind: 'turns', turns: chunk, cost: chunk.length });
  for (const chunk of chunkArray(items, maxItems)) shards.push({ kind: 'items', items: chunk, cost: chunk.length });
  return { cursor, shards, turnCount: turns.length, itemCount: items.length };
}

const asSnapshotFrame = (changeType: string, payload: unknown): ThreadChangeFrame =>
  ({ event: 'thread_change', seq: 0, change_type: changeType, data: payload as Record<string, unknown> });

function resetSnapshotTarget(state: ChatThreadState): void {
  const side = sideState(state);
  side.orderCounter = 0;
  side.extraTails.clear();
  // Replace contents in place: callers hold references to the maps.
  state.turns.clear();
  state.items.clear();
  state.tails.clear();
}

function applySnapshotTurn(state: ChatThreadState, entry: ChatThreadSnapshotTurn): void {
  const turnId = firstString(entry.turn_id);
  if (!turnId) return;
  state.turns.set(turnId, {
    turnId,
    rootTurnId: firstString(entry.root_turn_id),
    stopReason: firstString(entry.payload?.stop_reason),
    userRound: firstNumber(entry.user_round, entry.user_turn_index, entry.payload?.user_round) ?? null,
    status: firstString(entry.status) ?? null,
    clientMessageId: firstString(entry.client_message_id, entry.payload?.client_message_id) ?? null,
    userContent: typeof entry.content === 'string' ? entry.content : null
  });
}

function applySnapshotShard(state: ChatThreadState, shard: ChatThreadSnapshotShard): void {
  if (shard.kind === 'turns') {
    for (const entry of shard.turns ?? []) applySnapshotTurn(state, entry);
    return;
  }
  if (shard.kind === 'items') {
    // Reuse the durable appliers so items rebuild exactly as if replayed;
    // seq is unread inside the appliers.
    for (const payload of shard.items ?? []) applyItemUpsert(state, asSnapshotFrame('item_upsert', payload));
    return;
  }
}

function isPlanApplicable(state: ChatThreadState, plan: ChatThreadSnapshotPlan | null | undefined): boolean {
  if (!plan || typeof plan.cursor !== 'number' || !Number.isFinite(plan.cursor)) return false;
  return plan.cursor >= state.lastSeq;
}

/** Synchronous single-task apply of a prepared plan (compat / fallback path). */
export function applyChatThreadSnapshotPlanSync(
  state: ChatThreadState,
  plan: ChatThreadSnapshotPlan,
  now: number
): ChatThreadApplyResult {
  void now;
  if (!isPlanApplicable(state, plan)) return UNCHANGED; // stale full reload: cursor guard
  resetSnapshotTarget(state);
  for (const shard of plan.shards) applySnapshotShard(state, shard);
  state.lastSeq = plan.cursor;
  return CHANGED_STRUCTURAL;
}

export function applyChatThreadSnapshot(
  state: ChatThreadState,
  snapshot: ChatThreadSnapshot,
  // Signature symmetry with applyChatThreadFrame; snapshots carry no gap to age.
  now: number
): ChatThreadApplyResult {
  void now;
  if (!snapshot || typeof snapshot.cursor !== 'number' ||
    !Number.isFinite(snapshot.cursor) || snapshot.cursor < state.lastSeq) {
    return UNCHANGED; // stale full reload: cursor guard
  }
  const startedAt = perfNowMs();
  try {
    return applyChatThreadSnapshotPlanSync(state, planChatThreadSnapshot(snapshot), now);
  } finally {
    chatPerf.recordSnapshotApply(perfNowMs() - startedAt);
  }
}

export interface ChatThreadSnapshotApplyOptions {
  /** Cooperative cancellation; checked before each shard. */
  shouldAbort?: () => boolean;
  /** Main-thread yield used between shards (default: a 0ms macrotask). */
  yieldToMain?: () => Promise<void>;
  /** Accumulated shard cost allowed before yielding (default 512). */
  yieldBudget?: number;
}

export interface ChatThreadSnapshotApplyOutcome {
  applied: boolean;
  aborted: boolean;
  shardsApplied: number;
  shardCount: number;
  yields: number;
}

const defaultYieldToMain = (): Promise<void> =>
  new Promise<void>((resolve) => { setTimeout(resolve, 0); });

/**
 * Sharded, abortable apply of a prepared plan. The plan is applied in place; a
 * mid-flight abort stops before the next shard and leaves state.lastSeq
 * unadvanced, so the caller can reject the partial rebuild and re-request.
 */
export async function applyChatThreadSnapshotPlanSharded(
  state: ChatThreadState,
  plan: ChatThreadSnapshotPlan,
  options: ChatThreadSnapshotApplyOptions = {}
): Promise<ChatThreadSnapshotApplyOutcome> {
  const shardCount = plan?.shards?.length ?? 0;
  if (!isPlanApplicable(state, plan)) {
    return { applied: false, aborted: false, shardsApplied: 0, shardCount, yields: 0 };
  }
  const shouldAbort = options.shouldAbort;
  if (shouldAbort?.()) {
    return { applied: false, aborted: true, shardsApplied: 0, shardCount, yields: 0 };
  }
  const yieldToMain = options.yieldToMain ?? defaultYieldToMain;
  const yieldBudget = Math.max(1, options.yieldBudget ?? CHAT_THREAD_SNAPSHOT_YIELD_BUDGET);
  // W4-L: one measurement for the whole sharded pass. The work spans multiple
  // macrotasks (yields), so the span is opened once here and closed in `finally`
  // rather than per shard.
  const startedAt = perfNowMs();
  try {
    resetSnapshotTarget(state);
    let shardsApplied = 0;
    let yields = 0;
    let spent = 0;
    for (const shard of plan.shards) {
      if (shouldAbort?.()) {
        return { applied: false, aborted: true, shardsApplied, shardCount, yields };
      }
      applySnapshotShard(state, shard);
      shardsApplied += 1;
      spent += shard.cost || 1;
      if (spent >= yieldBudget && shardsApplied < shardCount) {
        spent = 0;
        yields += 1;
        await yieldToMain();
      }
    }
    state.lastSeq = plan.cursor;
    return { applied: true, aborted: false, shardsApplied, shardCount, yields };
  } finally {
    chatPerf.recordSnapshotApply(perfNowMs() - startedAt);
  }
}

// ---------------------------------------------------------------------------
// Optimistic turn binding (stream_started ack, §3.4 rule 5)
// ---------------------------------------------------------------------------

function findTurnByClientMessageId(state: ChatThreadState, clientMessageId: string): ThreadTurnState | null {
  // Called once per send, not per frame — a linear scan is fine.
  for (const turn of state.turns.values()) {
    if (turn.clientMessageId === clientMessageId) return turn;
  }
  return null;
}

/**
 * Bind the optimistic turn to its durable identity (plan §3.2 rule 5). The
 * ack only carries the client_message_id → turn_id mapping; bubble content
 * never comes from the ack. `userContent` is reserved for the durable user
 * item_upsert payload (queued submissions bind through it).
 */
export function bindStreamStarted(
  state: ChatThreadState,
  ack: StreamStartedAck,
  userContent?: string
): ThreadTurnState {
  const turnId = ack.turn_id;
  const clientMessageId = typeof ack.client_message_id === 'string' && ack.client_message_id
    ? ack.client_message_id
    : null;
  let optimistic = clientMessageId ? findTurnByClientMessageId(state, clientMessageId) : null;
  const canonical = state.turns.get(turnId) ?? null;

  if (optimistic && canonical && optimistic !== canonical) {
    // The canonical turn raced ahead of the ack (resume replay): fold the
    // optimistic placeholders into it and drop the optimistic entry.
    if (canonical.userContent === null && optimistic.userContent !== null) {
      canonical.userContent = optimistic.userContent;
    }
    if (canonical.userRound === null && optimistic.userRound !== null) {
      canonical.userRound = optimistic.userRound;
    }
    state.turns.delete(optimistic.turnId);
    optimistic = null;
  }

  let turn: ThreadTurnState;
  if (optimistic) {
    if (optimistic.turnId !== turnId) {
      state.turns.delete(optimistic.turnId);
      optimistic.turnId = turnId;
    }
    turn = optimistic;
  } else if (canonical) {
    turn = canonical;
  } else {
    turn = { turnId, userRound: null, status: null, clientMessageId, userContent: null };
  }

  // Ack values are server truth for the optimistic round. lastSeq stays
  // untouched: resume_from_seq frames are replayed idempotently by the
  // watch/resume path.
  if (typeof ack.user_round === 'number' && Number.isFinite(ack.user_round)) turn.userRound = ack.user_round;
  if (typeof userContent === 'string' && userContent) turn.userContent = userContent;
  if (clientMessageId && turn.clientMessageId === null) turn.clientMessageId = clientMessageId;
  state.turns.set(turnId, turn);
  return turn;
}

// ---------------------------------------------------------------------------
// Read helpers (render-time)
// ---------------------------------------------------------------------------

export function getThreadTurn(state: ChatThreadState, turnId: string): ThreadTurnState | undefined {
  return state.turns.get(turnId);
}

export function getThreadItem(state: ChatThreadState, itemId: string): ThreadItemState | undefined {
  return state.items.get(itemId);
}

export function getThreadTail(state: ChatThreadState, itemId: string): ThreadTailState | undefined {
  return state.tails.get(itemId);
}

/** Completed item text followed by any live suffix beyond its prefix. */
export function composeItemText(state: ChatThreadState, itemId: string, field: ChatThreadField): string {
  const item = state.items.get(itemId);
  const text = itemFieldText(item, field);
  const tail = readFieldTail(state, itemId, field);
  return tail?.text ? text + tail.text.slice(Math.max(0, text.length - tail.offset)) : text;
}
