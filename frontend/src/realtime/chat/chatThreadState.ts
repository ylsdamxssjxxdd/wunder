// Chat streaming pipeline v2 — client state layer (pure reducer).
// Spec: docs/聊天流式管线根治方案.md §3.3 (frame protocol), §3.4 (state machine
// rules), §5.5 (bounded buffers). Pure TypeScript: no Vue/Pinia imports; every
// mutation happens in place on the ChatThreadState maps and the caller owns
// reactive invalidation.
//
// Side store: ChatThreadState is a frozen cross-module contract
// (chatThreadTypes.ts) with no slot for three reducer internals, so they live
// in a WeakMap keyed by the state instance (per-session, GC'd with the state,
// reset by applyChatThreadSnapshot):
//   1. the monotonic ThreadItemState.order counter — items.size cannot derive
//      it because the items map shrinks on snapshot replacement and would
//      repeat values;
//   2. the per-(item_id, field) max block end offset — keeps every tail
//      decision O(1) instead of scanning the block map;
//   3. tails for fields other than content/reasoning (tool/command output),
//      which ThreadTailState cannot represent.
//
// ChatThreadApplyResult.droppedEphemeral counts ephemeral tail frames the
// reducer discarded (offset mismatch against the current tail position).

import { THREAD_GAP_MAX_FRAMES, THREAD_GAP_MAX_MS, flattenThreadItemRow, hasEmbeddedItemPayload } from './chatThreadTypes';
import type {
  ChatThreadApplyResult,
  ChatThreadField,
  ChatThreadFrame,
  ChatThreadState,
  StreamStartedAck,
  ThreadBlockState,
  ThreadBlockUpsertPayload,
  ThreadChangeFrame,
  ThreadGapEntry,
  ThreadItemState,
  ThreadItemTailFrame,
  ThreadItemUpsertPayload,
  ThreadTailState,
  ThreadTurnState,
  ThreadTurnStatusPayload,
  ThreadTurnUpsertPayload
} from './chatThreadTypes';

export interface ChatThreadSnapshotTurn {
  turn_id: string;
  user_round?: number;
  status?: string;
  content?: string;
  client_message_id?: string;
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
  blocks: ThreadBlockUpsertPayload[];
}

// Shared verdict objects: the reducer never mutates results, so hot paths can
// return these without allocating.
const UNCHANGED: ChatThreadApplyResult = { changed: false, structural: false, needResume: false, droppedEphemeral: 0 };
const CHANGED_CONTENT: ChatThreadApplyResult = { changed: true, structural: false, needResume: false, droppedEphemeral: 0 };
const CHANGED_STRUCTURAL: ChatThreadApplyResult = { changed: true, structural: true, needResume: false, droppedEphemeral: 0 };
const DROPPED_TAIL: ChatThreadApplyResult = { changed: false, structural: false, needResume: false, droppedEphemeral: 1 };
const RESUME_REQUIRED: ChatThreadApplyResult = { changed: false, structural: false, needResume: true, droppedEphemeral: 0 };

interface FieldTail {
  offset: number;
  text: string;
}

interface ChatThreadSideState {
  orderCounter: number;
  /** Max block end per `${itemId}\u0000${field}`; O(1) tail positioning. */
  blockEnds: Map<string, number>;
  /** Tails for fields outside content/reasoning (tool/command output). */
  extraTails: Map<string, FieldTail>;
}

const sideStates = new WeakMap<ChatThreadState, ChatThreadSideState>();

function sideState(state: ChatThreadState): ChatThreadSideState {
  let side = sideStates.get(state);
  if (!side) {
    side = { orderCounter: 0, blockEnds: new Map(), extraTails: new Map() };
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

function mergeResults(a: ChatThreadApplyResult, b: ChatThreadApplyResult): ChatThreadApplyResult {
  if (a === UNCHANGED) return b;
  if (b === UNCHANGED) return a;
  return {
    changed: a.changed || b.changed,
    structural: a.structural || b.structural,
    needResume: a.needResume || b.needResume,
    droppedEphemeral: a.droppedEphemeral + b.droppedEphemeral
  };
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
  now: number
): ChatThreadApplyResult {
  switch (frame.event) {
    case 'thread_change':
      return applyChangeFrame(state, frame, now);
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
  now: number
): ChatThreadApplyResult {
  const seq = frame.seq;
  if (typeof seq !== 'number' || !Number.isFinite(seq)) return UNCHANGED;
  if (seq <= state.lastSeq) return UNCHANGED; // idempotent replay
  if (seq > state.lastSeq + 1) {
    insertGapEntry(state, frame, now);
    return gapOverflowResult(state, now);
  }

  let result = applyInOrderChange(state, frame);
  state.lastSeq = seq;
  let entry = takeContiguousGapEntry(state);
  while (entry) {
    result = mergeResults(result, applyInOrderChange(state, entry.frame));
    state.lastSeq = entry.frame.seq;
    entry = takeContiguousGapEntry(state);
  }
  // Far-ahead frames may remain buffered; size/age overflow still applies.
  return mergeResults(result, gapOverflowResult(state, now));
}

function applyInOrderChange(state: ChatThreadState, frame: ThreadChangeFrame): ChatThreadApplyResult {
  switch (frame.change_type) {
    case 'item_upsert':
      return applyItemUpsert(state, frame);
    case 'text_block':
      return applyTextBlock(state, frame);
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
// Gap buffer (bounded per §5.5: 64 frames / 1000ms)
// ---------------------------------------------------------------------------

function insertGapEntry(state: ChatThreadState, frame: ThreadChangeFrame, now: number): void {
  const gap = state.gap;
  // The gap is bounded (≤ 64 entries before the overflow check), so a linear
  // scan from the tail is cheaper than maintaining a binary search.
  for (let i = gap.length - 1; i >= 0; i -= 1) {
    const seq = gap[i].frame.seq;
    if (seq === frame.seq) return; // duplicate delivery
    if (seq < frame.seq) {
      gap.splice(i + 1, 0, { frame, receivedAt: now });
      return;
    }
  }
  gap.unshift({ frame, receivedAt: now });
}

function takeContiguousGapEntry(state: ChatThreadState): ThreadGapEntry | null {
  const head = state.gap[0];
  if (head && head.frame.seq === state.lastSeq + 1) {
    state.gap.shift();
    return head;
  }
  return null;
}

function gapOverflowResult(state: ChatThreadState, now: number): ChatThreadApplyResult {
  const gap = state.gap;
  const overflowed = gap.length > THREAD_GAP_MAX_FRAMES ||
    (gap.length > 0 && now - gap[0].receivedAt > THREAD_GAP_MAX_MS);
  if (!overflowed) return UNCHANGED;
  gap.length = 0;
  return RESUME_REQUIRED;
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
      reasoning: typeof data.reasoning === 'string' ? data.reasoning : '',
      order: nextItemOrder(state),
      raw: data
    };
    state.items.set(itemId, item);
    // A terminal item carries the turn outcome; admin-visibility items are
    // stored normally here, exclusion is the projection's job.
    if (item.kind === 'terminal' && item.status) syncTerminalTurnStatus(state, item);
    return CHANGED_STRUCTURAL;
  }

  // Same revision is idempotent; a lower revision is stale.
  if (revision <= existing.revision) return UNCHANGED;
  let structural = false;
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
  if (typeof data.content === 'string') existing.content = data.content;
  if (typeof data.reasoning === 'string') existing.reasoning = data.reasoning;
  const role = firstString(data.role);
  if (role !== undefined) existing.role = role;
  const visibility = firstString(data.visibility);
  if (visibility !== undefined) existing.visibility = visibility;
  if (existing.kind === 'terminal' && status !== undefined) syncTerminalTurnStatus(state, existing);
  return structural ? CHANGED_STRUCTURAL : CHANGED_CONTENT;
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

function applyTextBlock(state: ChatThreadState, frame: ThreadChangeFrame): ChatThreadApplyResult {
  // Wire payloads are unvalidated JSON; casts go through unknown.
  const data = frame.data as unknown as ThreadBlockUpsertPayload;
  const itemId = firstString(data.item_id, frame.item_id);
  if (!itemId) return UNCHANGED;
  const field = firstString(data.field) ?? 'content';
  const blockIndex = firstNumber(data.block_index);
  if (blockIndex === undefined) return UNCHANGED;
  // content_offset / reasoning_offset are mutually exclusive; `field` picks
  // the side that applies.
  const offset = field === 'reasoning'
    ? firstNumber(data.reasoning_offset, data.content_offset) ?? 0
    : firstNumber(data.content_offset, data.reasoning_offset) ?? 0;
  const rawText = field === 'reasoning' ? data.reasoning : data.content;
  const text = typeof rawText === 'string' ? rawText : '';

  let fieldBlocks = state.blocks.get(itemId);
  if (!fieldBlocks) {
    fieldBlocks = new Map();
    state.blocks.set(itemId, fieldBlocks);
  }
  let indexBlocks = fieldBlocks.get(field);
  if (!indexBlocks) {
    indexBlocks = new Map();
    fieldBlocks.set(field, indexBlocks);
  }
  const previous = indexBlocks.get(blockIndex);
  if (previous && previous.offset === offset && previous.text === text) return UNCHANGED; // snapshot idempotent
  indexBlocks.set(blockIndex, { itemId, field, blockIndex, offset, text });

  // Advance the block-end watermark and heal the ephemeral tail: everything
  // the durable snapshot now covers must no longer be served from the tail.
  const side = sideState(state);
  const coordinate = tailCoordinate(itemId, field);
  const end = offset + text.length;
  if (end > (side.blockEnds.get(coordinate) ?? 0)) {
    side.blockEnds.set(coordinate, end);
    truncateFieldTail(state, itemId, field, end);
  }
  return CHANGED_CONTENT;
}

function applyTurnUpsert(state: ChatThreadState, frame: ThreadChangeFrame): ChatThreadApplyResult {
  const data = frame.data as unknown as ThreadTurnUpsertPayload;
  const turnId = firstString(data.turn_id, frame.turn_id);
  if (!turnId) return UNCHANGED;
  const existing = state.turns.get(turnId);

  if (!existing) {
    state.turns.set(turnId, {
      turnId,
      userRound: firstNumber(data.user_round) ?? null,
      status: firstString(data.status) ?? null,
      clientMessageId: firstString(data.client_message_id) ?? null,
      userContent: typeof data.content === 'string' ? data.content : null
    });
    return CHANGED_STRUCTURAL;
  }

  let changed = false;
  let structural = false;
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
      status,
      clientMessageId: null,
      userContent: null
    });
    return CHANGED_STRUCTURAL;
  }
  if (existing.status === status) return UNCHANGED;
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

function truncateFieldTail(state: ChatThreadState, itemId: string, field: ChatThreadField, blockEnd: number): void {
  if (field === 'content' || field === 'reasoning') {
    const tail = state.tails.get(itemId);
    if (!tail) return;
    if (field === 'content') {
      if (tail.contentOffset < blockEnd) {
        tail.content = tail.content.slice(blockEnd - tail.contentOffset);
        tail.contentOffset = blockEnd;
      }
      return;
    }
    if (tail.reasoningOffset < blockEnd) {
      tail.reasoning = tail.reasoning.slice(blockEnd - tail.reasoningOffset);
      tail.reasoningOffset = blockEnd;
    }
    return;
  }
  const stored = sideState(state).extraTails.get(tailCoordinate(itemId, field));
  if (stored && stored.offset < blockEnd) {
    stored.text = stored.text.slice(blockEnd - stored.offset);
    stored.offset = blockEnd;
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
  // let the following durable text_block heal the text. Missing base_seq is
  // treated as 0 for legacy senders.
  const baseSeq = typeof frame.base_seq === 'number' && Number.isFinite(frame.base_seq)
    ? frame.base_seq
    : 0;
  if (state.lastSeq < baseSeq) return DROPPED_TAIL;

  const current = readFieldTail(state, itemId, field);
  // An empty stored tail (fresh field slot or fully healed by blocks) restarts
  // from the durable block-end watermark.
  const side = sideState(state);
  const position = current && current.text
    ? current.offset + current.text.length
    : side.blockEnds.get(tailCoordinate(itemId, field)) ?? 0;
  if (frame.offset !== -1 && frame.offset !== position) {
    // offset < position: already covered by a block snapshot or an earlier
    // append; offset > position: hole — wait for the next block snapshot.
    return DROPPED_TAIL;
  }
  const baseOffset = current && current.text ? current.offset : position;
  writeFieldTail(state, itemId, field, baseOffset, (current ? current.text : '') + text);
  return CHANGED_CONTENT;
}

// ---------------------------------------------------------------------------
// Snapshot (full reload with cursor guard, §3.4 rule 4)
// ---------------------------------------------------------------------------

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

  const side = sideState(state);
  side.orderCounter = 0;
  side.blockEnds.clear();
  side.extraTails.clear();

  // Replace contents in place: callers hold references to the maps.
  state.turns.clear();
  for (const entry of snapshot.turns ?? []) {
    const turnId = firstString(entry.turn_id);
    if (!turnId) continue;
    state.turns.set(turnId, {
      turnId,
      userRound: firstNumber(entry.user_round) ?? null,
      status: firstString(entry.status) ?? null,
      clientMessageId: firstString(entry.client_message_id) ?? null,
      userContent: typeof entry.content === 'string' ? entry.content : null
    });
  }

  state.items.clear();
  state.blocks.clear();
  state.tails.clear();
  // Reuse the durable appliers against the cleared maps so items and blocks
  // rebuild exactly as if replayed; seq is unread inside the appliers.
  const asFrame = (changeType: string, payload: unknown): ThreadChangeFrame =>
    ({ event: 'thread_change', seq: 0, change_type: changeType, data: payload as Record<string, unknown> });
  for (const entry of snapshot.items ?? []) {
    // Atomic-snapshot rows carry an embedded payload copy next to the row
    // columns; flat projections pass through untouched.
    const payload = hasEmbeddedItemPayload(entry as Record<string, unknown>)
      ? flattenThreadItemRow(entry as Record<string, unknown>)
      : entry;
    applyItemUpsert(state, asFrame('item_upsert', payload));
  }
  for (const payload of snapshot.blocks ?? []) {
    applyTextBlock(state, asFrame('text_block', payload));
  }

  state.gap.length = 0;
  state.lastSeq = snapshot.cursor;
  return CHANGED_STRUCTURAL;
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

/**
 * Authoritative text for one item field: blocks joined by offset order, plus
 * the tail portion beyond the last block end. Render-time only — O(#blocks).
 * Falls back to the item payload when the field has neither blocks nor tail.
 */
export function composeItemText(state: ChatThreadState, itemId: string, field: ChatThreadField): string {
  const indexBlocks = state.blocks.get(itemId)?.get(field);
  let text = '';
  let lastEnd = 0;
  if (indexBlocks && indexBlocks.size > 0) {
    const blocks: ThreadBlockState[] = Array.from(indexBlocks.values()).sort((a, b) => a.offset - b.offset);
    for (const block of blocks) {
      text += block.text;
      lastEnd = Math.max(lastEnd, block.offset + block.text.length);
    }
  }
  const tail = readFieldTail(state, itemId, field);
  if (tail && tail.text) {
    // Keep only the part beyond the last durable block; a tail that starts at
    // or after it appends whole (a hole is accepted — the server never
    // produces one).
    text += tail.text.slice(Math.max(0, lastEnd - tail.offset));
  }
  if (!text) {
    const item = state.items.get(itemId);
    if (item) text = field === 'reasoning' ? item.reasoning : item.content;
  }
  return text;
}
