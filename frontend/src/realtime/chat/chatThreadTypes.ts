// Thread streaming pipeline v2 — protocol and state contracts.
// Spec: docs/聊天流式管线根治方案.md §3.3 (frames) and §3.4 (state/reducer rules).
// This module is the single source of truth shared by chatThreadState and
// chatThreadProjection; both the reducer and the projection must stay pure.

export type ChatThreadField = 'content' | 'reasoning' | string;

// ---------------------------------------------------------------------------
// Server frames (protocol v2)
// ---------------------------------------------------------------------------

/** Durable frame. Canonical completed-history projection; replayable. */
export interface ThreadChangeFrame {
  event: 'thread_change';
  /** Exclusive canonical rollout cursor; values can have gaps. */
  seq: number;
  change_type: 'item_upsert' | 'turn_upsert' | 'turn_status' | string;
  item_id?: string;
  turn_id?: string;
  revision?: number;
  data: Record<string, unknown>;
}

/** Ephemeral low-latency tail append. Droppable; replaced by the completed item. */
export interface ThreadItemTailFrame {
  event: 'thread_item_tail';
  item_id: string;
  field: ChatThreadField;
  /** UTF-16 code-unit offset of `text` inside the item field.
   *  -1 means "arrival-order append" (tool/command output without durable offsets). */
  offset: number;
  /** Durable seq the tail builds on (plan §2 I3): only applied when
   *  state.lastSeq >= base_seq. Missing base_seq is treated as 0 (legacy). */
  base_seq?: number;
  text: string;
}

/** Durable cursor was trimmed server-side; an atomic snapshot is required. */
export interface ThreadSnapshotRequiredFrame {
  event: 'thread_snapshot_required';
  seq?: number;
  /** Client must be able to rebuild history from at least this seq. */
  required_from_seq?: number;
  /** Oldest change the server can still serve. */
  earliest_available_seq?: number;
}

/** Server stopped pushing (slow client); reconnect watch from `cursor`. */
export interface ThreadOverflowFrame {
  event: 'stream_overflow';
  session_id?: string;
  cursor?: number;
  resume_recommended?: boolean;
}

/** First reply of a `start` request: binds the optimistic turn to durable state.
 *  Only the client_message_id → turn_id binding is honored; bubble content is
 *  never taken from the ack (plan §3.2). */
export interface StreamStartedAck {
  event: 'stream_started';
  request_id?: string;
  turn_id: string;
  user_round?: number;
  /** Session change cursor as of turn acceptance (informational; the reducer
   *  keeps its own lastSeq and replays the accept-time frames idempotently). */
  resume_from_seq: number;
  client_message_id?: string;
}

export type ChatThreadFrame =
  | ThreadChangeFrame
  | ThreadItemTailFrame
  | ThreadSnapshotRequiredFrame
  | ThreadOverflowFrame;

export function isThreadChangeFrame(frame: unknown): frame is ThreadChangeFrame {
  return typeof frame === 'object' && frame !== null &&
    (frame as { event?: unknown }).event === 'thread_change';
}
export function isThreadItemTailFrame(frame: unknown): frame is ThreadItemTailFrame {
  return typeof frame === 'object' && frame !== null &&
    (frame as { event?: unknown }).event === 'thread_item_tail';
}
export function isThreadSnapshotRequiredFrame(frame: unknown): frame is ThreadSnapshotRequiredFrame {
  return typeof frame === 'object' && frame !== null &&
    (frame as { event?: unknown }).event === 'thread_snapshot_required';
}
export function isStreamStartedAck(frame: unknown): frame is StreamStartedAck {
  return typeof frame === 'object' && frame !== null &&
    (frame as { event?: unknown }).event === 'stream_started';
}

// ---------------------------------------------------------------------------
// Change payloads (threads through thread_change.data)
// ---------------------------------------------------------------------------

/** change_type === 'item_upsert' — projection of one thread_items row. */
export interface ThreadItemUpsertPayload {
  item_id: string;
  turn_id: string;
  model_round?: number;
  user_round?: number;
  kind: string;
  status?: string;
  revision?: number;
  role?: string;
  visibility?: string;
  content?: string;
  reasoning?: string;
  [key: string]: unknown;
}

/** change_type === 'turn_upsert' — turn accepted (user bubble content included). */
export interface ThreadTurnUpsertPayload {
  stop_reason?: string | null;
  root_turn_id?: string;
  turn_id: string;
  user_round?: number;
  status?: string;
  content?: string;
  client_message_id?: string;
}

/** change_type === 'turn_status' — turn lifecycle transition. */
export interface ThreadTurnStatusPayload {
  stop_reason?: string | null;
  turn_id: string;
  status: string;
}

/** Control-frame data for thread_snapshot_required (plan §3.1). */
export interface ThreadSnapshotRequiredData {
  required_from_seq?: number;
  earliest_available_seq?: number;
}

/**
 * Merge an immutable thread_items row (projection columns + embedded payload
 * copy) into the flat payload object the reducer consumes. Row columns are
 * authoritative over the embedded copy.
 */
export function flattenThreadItemRow(row: Record<string, unknown>): Record<string, unknown> {
  const nested = row.payload;
  const payload = nested && typeof nested === 'object' && !Array.isArray(nested)
    ? nested as Record<string, unknown>
    : {};
  return {
    ...payload,
    item_id: row.item_id ?? payload.item_id,
    turn_id: row.turn_id ?? payload.turn_id,
    root_turn_id: row.root_turn_id ?? payload.root_turn_id,
    created_seq: row.created_seq ?? payload.created_seq,
    kind: row.kind ?? payload.kind,
    status: row.status ?? payload.status,
    revision: row.revision ?? payload.revision,
    visibility: row.visibility ?? payload.visibility
  };
}

/** True when the object carries an embedded payload copy (row-shaped). */
export function hasEmbeddedItemPayload(row: Record<string, unknown>): boolean {
  const nested = row.payload;
  return Boolean(nested && typeof nested === 'object' && !Array.isArray(nested));
}

// ---------------------------------------------------------------------------
// Client state
// ---------------------------------------------------------------------------

export interface ThreadTurnState {
  stopReason?: string;
  rootTurnId?: string;
  turnId: string;
  userRound: number | null;
  status: string | null;
  clientMessageId: string | null;
  userContent: string | null;
}

export interface ThreadItemState {
  itemId: string;
  turnId: string;
  modelRound: number;
  kind: string;
  status: string;
  revision: number;
  role: string | null;
  visibility: string | null;
  /** Authoritative full text from the last item_upsert (may lag the ephemeral live tail). */
  content: string;
  reasoning: string;
  /** Monotonic local registration order; projection fallback ordering only. */
  order: number;
  raw: Record<string, unknown>;
}

export interface ThreadTailState {
  contentOffset: number;
  reasoningOffset: number;
  content: string;
  reasoning: string;
}

export interface ChatThreadState {
  sessionId: string;
  lastSeq: number;
  turns: Map<string, ThreadTurnState>;
  items: Map<string, ThreadItemState>;
  tails: Map<string, ThreadTailState>;
}

/** Reducer verdict for one applied frame. */
export interface ChatThreadApplyResult {
  changed: boolean;
  /** True when list topology changed (items/turns added/removed, status flips). */
  structural: boolean;
  droppedEphemeral: number;
}

// Repeated overflow controls share one resume cooldown.
export const THREAD_OVERFLOW_RESUME_COOLDOWN_MS = 800;

export function emptyChatThreadState(sessionId: string): ChatThreadState {
  return {
    sessionId,
    lastSeq: 0,
    turns: new Map(),
    items: new Map(),
    tails: new Map(),
  };
}
