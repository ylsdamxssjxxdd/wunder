// AI生成
// Live context occupancy observed on the websocket. Usage frames are not
// timeline items and are dropped by the display projection, so the latest
// observed snapshot is kept per session for the composer to read while a long
// turn is still running.
import { shallowRef } from 'vue';

export type ChatLiveContextUsage = {
  used: number;
  total: number | null;
  seq: number | null;
};

const MAX_SESSIONS = 64;
const LIVE_CONTEXT_EVENT_TYPES = new Set([
  'context_usage',
  'token_usage',
  'round_usage',
  'model_usage',
  'llm_output',
  'final',
  'compaction'
]);

const entries = new Map<string, ChatLiveContextUsage>();
const sessionClocks = shallowRef<Record<string, number>>({});

const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;

const normalizeCount = (value: unknown): number | null => {
  if (typeof value === 'number') {
    return Number.isFinite(value) && value >= 0 ? Math.round(value) : null;
  }
  if (typeof value !== 'string') return null;
  const text = value.trim();
  if (!text) return null;
  const parsed = Number(text);
  return Number.isFinite(parsed) && parsed >= 0 ? Math.round(parsed) : null;
};

const firstCount = (...values: unknown[]): number | null => {
  for (const value of values) {
    const normalized = normalizeCount(value);
    if (normalized !== null) return normalized;
  }
  return null;
};

const firstPositiveCount = (...values: unknown[]): number | null => {
  for (const value of values) {
    const normalized = normalizeCount(value);
    if (normalized !== null && normalized > 0) return normalized;
  }
  return null;
};

const eventSource = (payload: unknown): Record<string, unknown> | null => {
  const record = asRecord(payload);
  if (!record) return null;
  const data = asRecord(record.data);
  return data && Object.keys(data).length > 0 ? data : record;
};

const resolveExplicitContextTokens = (source: Record<string, unknown>): number | null => {
  const contextUsage = asRecord(source.context_usage ?? source.contextUsage);
  const messageStats = asRecord(source.message_stats ?? source.messageStats);
  const statsUsage = asRecord(messageStats?.context_usage ?? messageStats?.contextUsage);
  return firstCount(
    source.context_occupancy_tokens,
    source.contextOccupancyTokens,
    contextUsage?.context_occupancy_tokens,
    contextUsage?.contextOccupancyTokens,
    source.context_tokens,
    source.contextTokens,
    contextUsage?.context_tokens,
    contextUsage?.contextTokens,
    messageStats?.context_occupancy_tokens,
    messageStats?.contextOccupancyTokens,
    messageStats?.contextTokens,
    messageStats?.context_tokens,
    statsUsage?.context_occupancy_tokens,
    statsUsage?.contextOccupancyTokens,
    statsUsage?.contextTokens,
    statsUsage?.context_tokens
  );
};

const resolveUsageContextTokens = (
  eventType: string,
  source: Record<string, unknown>
): number | null => {
  if (eventType !== 'llm_output' && eventType !== 'token_usage') return null;
  const usage = asRecord(source.usage ?? source.round_usage ?? source.roundUsage);
  if (!usage || source.estimated === true || usage.estimated === true) return null;
  return firstPositiveCount(usage.input_tokens, usage.inputTokens, usage.input);
};

const resolveCompactionContextTokens = (
  eventType: string,
  source: Record<string, unknown>
): number | null => {
  if (eventType !== 'compaction') return null;
  const status = String(source.status ?? source.result ?? '').trim().toLowerCase();
  if (status && status !== 'done' && status !== 'fallback') return null;
  return firstPositiveCount(
    source.context_tokens_after,
    source.observed_context_tokens_after,
    source.projected_request_tokens_after
  );
};

const resolveContextTotalTokens = (source: Record<string, unknown>): number | null => {
  const contextUsage = asRecord(source.context_usage ?? source.contextUsage);
  const messageStats = asRecord(source.message_stats ?? source.messageStats);
  return firstPositiveCount(
    source.max_context,
    source.maxContext,
    source.context_total_tokens,
    source.contextTotalTokens,
    source.context_window,
    source.context_max_tokens,
    contextUsage?.max_context,
    contextUsage?.maxContext,
    contextUsage?.context_max_tokens,
    contextUsage?.contextMaxTokens,
    messageStats?.contextTotalTokens,
    messageStats?.context_total_tokens,
    messageStats?.context_max_tokens,
    messageStats?.max_context
  );
};

const isUnobservedSource = (source: Record<string, unknown>): boolean =>
  String(source.context_usage_source ?? source.contextUsageSource ?? '')
    .trim()
    .toLowerCase()
    .startsWith('unobserved');

const resolveEventSeq = (source: Record<string, unknown>, payload: unknown): number | null => {
  const outer = asRecord(payload);
  return firstCount(
    source.event_seq,
    source.eventSeq,
    source.event_id,
    source.eventId,
    outer?.event_seq,
    outer?.eventSeq,
    outer?.event_id,
    outer?.eventId
  );
};

const notify = (sessionId: string): void => {
  sessionClocks.value = {
    ...sessionClocks.value,
    [sessionId]: Number(sessionClocks.value[sessionId] || 0) + 1
  };
};

export const applyChatLiveContextUsageEvent = (
  sessionId: string,
  eventType: string,
  payload: unknown
): void => {
  const key = String(sessionId || '').trim();
  const normalizedType = String(eventType || '').trim().toLowerCase();
  if (!key || !LIVE_CONTEXT_EVENT_TYPES.has(normalizedType)) return;
  const source = eventSource(payload);
  if (!source) return;
  const seq = resolveEventSeq(source, payload);
  const previous = entries.get(key);
  // Usage frames are cumulative snapshots; a replayed or reordered older frame
  // must never move the composer back to a previous value.
  if (previous && previous.seq !== null && seq !== null && seq < previous.seq) return;

  const explicitTokens = resolveExplicitContextTokens(source);
  const compactionTokens = resolveCompactionContextTokens(normalizedType, source);
  const fallbackTokens = resolveUsageContextTokens(normalizedType, source);
  let used = compactionTokens ?? explicitTokens ?? fallbackTokens;
  if (used === 0 && (isUnobservedSource(source) || normalizedType === 'compaction')) {
    // Compaction resets are explicitly unobserved until the next provider
    // usage arrives; zero here is "unknown", not an occupancy snapshot.
    used = null;
  }
  const total = resolveContextTotalTokens(source);
  const resolvedUsed = used ?? previous?.used;
  if (resolvedUsed === undefined) return;
  const next: ChatLiveContextUsage = {
    used: resolvedUsed,
    total: total ?? previous?.total ?? null,
    seq: seq ?? previous?.seq ?? null
  };
  if (
    previous &&
    previous.used === next.used &&
    previous.total === next.total &&
    previous.seq === next.seq
  ) {
    return;
  }

  entries.delete(key);
  entries.set(key, next);
  while (entries.size > MAX_SESSIONS) {
    const oldest = entries.keys().next().value;
    if (oldest === undefined) break;
    entries.delete(oldest);
  }
  notify(key);
};

export const readChatLiveContextUsage = (
  sessionId: string
): ChatLiveContextUsage | null => {
  const key = String(sessionId || '').trim();
  if (!key) return null;
  void sessionClocks.value[key];
  return entries.get(key) ?? null;
};

export const resetChatLiveContextUsage = (): void => {
  entries.clear();
  sessionClocks.value = {};
};
