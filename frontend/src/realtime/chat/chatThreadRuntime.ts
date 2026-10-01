// Durable chat pipeline hub: per-session thread state registry, server frame
// normalization and render invalidation. Chat has one protocol and one reducer.
import {
  applyChatThreadFrame,
  applyChatThreadSnapshot,
  bindStreamStarted,
  type ChatThreadSnapshot
} from './chatThreadState';
import { buildChatThreadRenderableMessages } from './chatThreadProjection';
import {
  emptyChatThreadState,
  flattenThreadItemRow,
  hasEmbeddedItemPayload,
  THREAD_OVERFLOW_RESUME_COOLDOWN_MS,
  type ChatThreadFrame,
  type ChatThreadState,
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

interface ChatThreadRuntimeEntry {
  key: string;
  state: ChatThreadState;
  /** Caller-owned persistent projection shell: keeps the materialize row cache alive across bumps. */
  renderProjection: ChatRuntimeProjection;
  /** Clock (ms) of the last resume dispatch; collapses control-frame bursts. */
  lastResumeDispatchAt: number;
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
      lastResumeDispatchAt: 0
    };
    registry.set(key, entry);
    if (registry.size > REGISTRY_LIMIT) {
      const oldest = registry.keys().next().value;
      if (oldest && oldest !== key) registry.delete(oldest);
    }
  }
  return entry;
};

export const getChatThreadState = (key: string): ChatThreadState | null =>
  registry.get(key)?.state ?? null;

export const isChatThreadV2Session = (key: string): boolean =>
  Boolean(registry.get(key)) && isChatChangeStreamEnabled();

export const resetChatThreadRuntime = (key: string): void => {
  registry.delete(key);
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
    // Bubble ids are deterministic: `titem:{item_id}` for assistant text.
    const messageId = `titem:${frame.item_id}`;
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
  now?: () => number;
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

const dispatchSnapshotRequired = (
  store: unknown,
  key: string,
  payload: unknown,
  hooks: ChatThreadServerEventHooks
): void => {
  const data = extractSnapshotRequiredData(payload);
  const loader = chatThreadSnapshotLoader;
  if (!loader) {
    hooks.onSnapshotRequired?.(data);
    return;
  }
  void (async () => {
    try {
      const snapshot = await loader(key);
      const entry = registry.get(key);
      if (!entry) return;
      const applied = applyChatThreadSnapshot(entry.state, snapshot, (hooks.now ?? Date.now)());
      if (!applied.changed) throw new Error('thread snapshot cursor is stale');
      markRuntimeProjectionChanged(store, { sessionId: key, reason: 'thread_structure' });
      hooks.onSnapshotApplied?.();
    } catch {
      hooks.onSnapshotRequired?.(data);
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
  if (CONTROL_EVENT.test(eventType)) return false;
  const entry = ensureChatThreadRuntime(key);
  if (eventType === 'thread_snapshot_required') {
    dispatchSnapshotRequired(store, key, payload, hooks);
    return true;
  }
  const frame = toChatThreadFrame(eventType, payload);
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
  if (frame.event === 'thread_change' && frame.change_type === 'item_upsert') {
    const clientMessageId = typeof frame.data.client_message_id === 'string'
      ? frame.data.client_message_id
      : '';
    if (clientMessageId && frame.data.kind === 'user_message') {
      // Queued submissions bind their optimistic bubble through the durable
      // user item instead of a stream ack; its content is server truth.
      bindStreamStarted(
        entry.state,
        {
          event: 'stream_started',
          turn_id: String(frame.turn_id || frame.data.turn_id || ''),
          resume_from_seq: frame.seq,
          client_message_id: clientMessageId
        },
        typeof frame.data.content === 'string' ? frame.data.content : undefined
      );
    }
  }
  const result = applyChatThreadFrame(entry.state, frame, (hooks.now ?? Date.now)());
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
export const buildChatThreadMaterializedMessages = (
  key: string
): ChatRuntimeMessageProjection[] | null => {
  const entry = registry.get(key);
  if (!entry) return null;
  const messages = buildChatThreadRenderableMessages(entry.state);
  const session = (entry.renderProjection.sessions as Record<string, unknown>)[key] as
    | { messages: ChatRuntimeMessageProjection[] }
    | undefined;
  if (session) session.messages = messages;
  return materializeChatRuntimeProjectionList(
    entry.renderProjection,
    key,
    messages,
    { trustProjectionVersions: true }
  ) as unknown as ChatRuntimeMessageProjection[];
};
