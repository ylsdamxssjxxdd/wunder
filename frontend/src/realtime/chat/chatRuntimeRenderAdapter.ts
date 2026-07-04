import {
  isRuntimeMessageActive,
  selectChatRuntimeSession,
  selectVisibleMessageProjections
} from './chatRuntimeSelectors';
import { resolveChatRuntimeRenderableKey } from './chatRuntimeMessageKeys';
import type {
  ChatRuntimeMessageProjection,
  ChatRuntimeMessageStatus,
  ChatRuntimeProjection
} from './chatRuntimeTypes';

type ChatMessageLike = Record<string, unknown>;

export type ChatRuntimeRenderableMessage = {
  key: string;
  sourceIndex: number;
  message: ChatMessageLike;
};

export type BuildChatRuntimeRenderableMessagesOptions = {
  projection: ChatRuntimeProjection | null | undefined;
  sessionId: unknown;
  shouldRenderMessage?: (message: ChatMessageLike) => boolean;
};

export type ChatRuntimeProjectionRenderMode = 'shadow' | 'projection';

export type ChatRuntimeRenderableSourceDecision = {
  source: 'projection';
  event: 'projection-source';
  inspectShadow: boolean;
};

const RENDER_STORAGE_KEYS = [
  'wunder:chat-runtime-render',
  'wunder_chat_runtime_render'
];
const RENDER_SHADOW_STORAGE_KEYS = [
  'wunder:chat-runtime-render-shadow',
  'wunder_chat_runtime_render_shadow'
];
const RENDER_TRUE_VALUES = new Set(['1', 'true', 'on', 'yes', 'debug']);
const RENDER_SHADOW_VALUES = new Set(['shadow', 'compare', 'dry-run', 'dryrun']);
const RENDER_PROJECTION_VALUES = new Set(['projection-debug', 'force-projection', 'projected-debug']);
const RENDER_SEARCH_KEYS = ['chat_runtime_render', 'chatRuntimeRender'];
const RENDER_SHADOW_SEARCH_KEYS = ['chat_runtime_render_shadow', 'chatRuntimeRenderShadow'];
const MATERIALIZED_MESSAGE_CACHE_SESSION_LIMIT = 64;
const MATERIALIZED_MESSAGE_CACHE_ENTRY_LIMIT = 5000;

type MaterializedMessageCacheEntry = {
  sourceRevision: string;
  materializedMutableRevision: string;
  message: ChatMessageLike;
  lastUsed: number;
};

type MaterializedSessionMessageCache = {
  byMessageId: Map<string, MaterializedMessageCacheEntry>;
  lastUsed: number;
};

const materializedMessageCache = new WeakMap<
  ChatRuntimeProjection,
  Map<string, MaterializedSessionMessageCache>
>();
const projectionObjectIdentity = new WeakMap<object, number>();
let materializedMessageCacheClock = 0;
let projectionObjectIdentityClock = 0;

export const isChatRuntimeProjectionRenderEnabled = (): boolean =>
  resolveChatRuntimeProjectionRenderMode() === 'projection';

export const isChatRuntimeProjectionRenderShadowEnabled = (): boolean =>
  resolveChatRuntimeProjectionRenderMode() === 'shadow' ||
  readRuntimeRenderNamedFlag(RENDER_SHADOW_STORAGE_KEYS) ||
  readRuntimeRenderNamedSearchFlag(RENDER_SHADOW_SEARCH_KEYS);

export const resolveChatRuntimeProjectionRenderMode = (): ChatRuntimeProjectionRenderMode => {
  const raw = readRuntimeRenderRawFlag();
  if (RENDER_PROJECTION_VALUES.has(raw)) return 'projection';
  if (RENDER_SHADOW_VALUES.has(raw)) return 'shadow';
  if (RENDER_TRUE_VALUES.has(raw) || raw === 'projection' || raw === 'projected') {
    return 'shadow';
  }
  if (readRuntimeRenderNamedFlag(RENDER_SHADOW_STORAGE_KEYS) || readRuntimeRenderNamedSearchFlag(RENDER_SHADOW_SEARCH_KEYS)) {
    return 'shadow';
  }
  // Default to projection rendering: the runtime projection carries strict
  // event_id / event_seq dedup, client_message_id optimistic-merge, snapshot
  // full-replace and turn-ordered message selection. The legacy array is kept
  // only as an input/debug surface, never as the rendered source of truth.
  return 'projection';
};

export const materializeChatRuntimeMessages = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown
): ChatMessageLike[] => {
  const projectedMessages = selectVisibleMessageProjections(projection, sessionId);
  const sessionCache = resolveMaterializedSessionMessageCache(projection, sessionId);
  const activeMessageIds = new Set<string>();
  const materialized = projectedMessages
    .map((message) => {
      activeMessageIds.add(message.id);
      return materializeChatRuntimeMessageWithCache(sessionCache, message);
    })
    .filter((message): message is ChatMessageLike => Boolean(message));
  pruneMaterializedSessionMessageCache(sessionCache, activeMessageIds);
  return materialized;
};

export const buildChatRuntimeRenderableMessages = (
  options: BuildChatRuntimeRenderableMessagesOptions
): ChatRuntimeRenderableMessage[] => {
  const materialized = materializeChatRuntimeMessages(options.projection, options.sessionId);
  const shouldRender = typeof options.shouldRenderMessage === 'function'
    ? options.shouldRenderMessage
    : () => true;
  return materialized.reduce<ChatRuntimeRenderableMessage[]>((acc, message) => {
    if (!shouldRender(message)) return acc;
    acc.push({
      key: resolveChatRuntimeMessageRenderKey(message),
      // Runtime-rendered messages already carry a stable message id. Keeping the
      // index fixed prevents streaming deltas from invalidating legacy cache keys.
      sourceIndex: 0,
      message
    });
    return acc;
  }, []);
};

export const hasChatRuntimeRenderSession = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown
): boolean => Boolean(selectChatRuntimeSession(projection, sessionId));

export const resolveChatRuntimeRenderableSourceDecision = (input: {
  renderMode: ChatRuntimeProjectionRenderMode;
  projectionCount: number;
  projectionSessionKnown: boolean;
  shadowEnabled?: boolean;
}): ChatRuntimeRenderableSourceDecision => {
  return {
    source: 'projection',
    event: 'projection-source',
    inspectShadow: input.renderMode === 'shadow' || input.shadowEnabled === true
  };
};

export const summarizeChatRuntimeRenderableMessages = (
  messages: ChatRuntimeRenderableMessage[]
): Record<string, unknown> => ({
  count: Array.isArray(messages) ? messages.length : 0,
  keys: (Array.isArray(messages) ? messages : [])
    .slice(0, 8)
    .map((item) => item.key),
  projectedCount: (Array.isArray(messages) ? messages : [])
    .filter((item) => Boolean(item.message?.__runtime_projected))
    .length
});

export const materializeChatRuntimeMessage = (
  message: ChatRuntimeMessageProjection | null | undefined
): ChatMessageLike | null => {
  if (!message || (message.role !== 'user' && message.role !== 'assistant')) {
    return null;
  }
  if (isSyntheticGreetingDisplay(message.display)) {
    return null;
  }

  const active = isRuntimeMessageActive(message.status);
  const base: ChatMessageLike = cloneDisplayProjection(message.display);
  base.role = message.role;
  base.content = message.content;
  base.reasoning = message.reasoning;
  base.created_at = firstText(message.createdAt, base.created_at, base.createdAt);
  base.message_id = firstText(message.id, base.message_id, base.messageId);
  base.runtime_status = message.status;
  base.__runtime_projected = true;
  base.__runtime_message_id = message.id;
  base.__runtime_user_turn_id = message.userTurnId;
  base.__runtime_model_turn_id = message.modelTurnId;
  base.__runtime_render_key = resolveChatRuntimeProjectionKey(message);

  if (message.role === 'assistant') {
    base.state = resolveAssistantLegacyState(message.status);
    base.stream_incomplete = active;
    base.workflowStreaming = resolveProjectedWorkflowStreaming(message);
    base.reasoningStreaming = active && Boolean(message.reasoning);
    base.failed = message.failed || message.status === 'failed';
    base.cancelled = message.cancelled || message.status === 'cancelled';
    base.workflowItems = cloneProjectionRecords(message.workflowItems, base.workflowItems);
    base.subagents = cloneProjectionRecords(message.subagents, base.subagents);
  }

  return base;
};

const materializeChatRuntimeMessageWithCache = (
  sessionCache: MaterializedSessionMessageCache | null,
  message: ChatRuntimeMessageProjection | null | undefined
): ChatMessageLike | null => {
  if (!sessionCache || !message?.id) {
    return materializeChatRuntimeMessage(message);
  }
  const sourceRevision = buildProjectionMessageMaterializationRevision(message);
  const cached = sessionCache.byMessageId.get(message.id);
  if (cached?.sourceRevision === sourceRevision) {
    cached.lastUsed = ++materializedMessageCacheClock;
    sessionCache.lastUsed = cached.lastUsed;
    if (
      isMaterializedMessageAligned(cached.message, message) &&
      cached.materializedMutableRevision === buildMaterializedMutableFieldsRevision(cached.message)
    ) {
      return cached.message;
    }
  }

  const materialized = materializeChatRuntimeMessage(message);
  if (!materialized) {
    sessionCache.byMessageId.delete(message.id);
    return null;
  }
  const lastUsed = ++materializedMessageCacheClock;
  sessionCache.byMessageId.set(message.id, {
    sourceRevision,
    materializedMutableRevision: buildMaterializedMutableFieldsRevision(materialized),
    message: materialized,
    lastUsed
  });
  sessionCache.lastUsed = lastUsed;
  return materialized;
};

const resolveMaterializedSessionMessageCache = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown
): MaterializedSessionMessageCache | null => {
  if (!projection || typeof projection !== 'object') return null;
  const key = firstText(sessionId) || '__unknown__';
  let projectionCache = materializedMessageCache.get(projection);
  if (!projectionCache) {
    projectionCache = new Map();
    materializedMessageCache.set(projection, projectionCache);
  }
  let sessionCache = projectionCache.get(key);
  if (!sessionCache) {
    sessionCache = {
      byMessageId: new Map(),
      lastUsed: ++materializedMessageCacheClock
    };
    projectionCache.set(key, sessionCache);
    pruneProjectionMaterializedSessionCaches(projectionCache);
  } else {
    sessionCache.lastUsed = ++materializedMessageCacheClock;
  }
  return sessionCache;
};

const pruneProjectionMaterializedSessionCaches = (
  projectionCache: Map<string, MaterializedSessionMessageCache>
): void => {
  if (projectionCache.size <= MATERIALIZED_MESSAGE_CACHE_SESSION_LIMIT) return;
  [...projectionCache.entries()]
    .sort((left, right) => left[1].lastUsed - right[1].lastUsed)
    .slice(0, projectionCache.size - MATERIALIZED_MESSAGE_CACHE_SESSION_LIMIT)
    .forEach(([sessionId]) => projectionCache.delete(sessionId));
};

const pruneMaterializedSessionMessageCache = (
  sessionCache: MaterializedSessionMessageCache | null,
  activeMessageIds: Set<string>
): void => {
  if (!sessionCache) return;
  for (const messageId of sessionCache.byMessageId.keys()) {
    if (!activeMessageIds.has(messageId)) {
      sessionCache.byMessageId.delete(messageId);
    }
  }
  if (sessionCache.byMessageId.size <= MATERIALIZED_MESSAGE_CACHE_ENTRY_LIMIT) return;
  [...sessionCache.byMessageId.entries()]
    .sort((left, right) => left[1].lastUsed - right[1].lastUsed)
    .slice(0, sessionCache.byMessageId.size - MATERIALIZED_MESSAGE_CACHE_ENTRY_LIMIT)
    .forEach(([messageId]) => sessionCache.byMessageId.delete(messageId));
};

const buildProjectionMessageMaterializationRevision = (
  message: ChatRuntimeMessageProjection
): string => [
    message.id,
    message.role,
    message.content,
    message.reasoning,
    message.status,
    message.createdAt,
    message.createdSeq,
    message.updatedSeq,
    message.userTurnId,
    message.modelTurnId,
    message.final,
    message.failed,
    message.cancelled,
    buildProjectionMetadataRevision(message.display),
    buildProjectionMetadataRevision(message.workflowItems),
    buildProjectionMetadataRevision(message.subagents)
  ].join('\u0001');

const buildProjectionMetadataRevision = (value: unknown): string => {
  if (!value || typeof value !== 'object') return '';
  try {
    return JSON.stringify(value) || '';
  } catch {
    return String(resolveProjectionObjectIdentity(value));
  }
};

const resolveProjectionObjectIdentity = (value: unknown): number => {
  if (!value || typeof value !== 'object') return 0;
  const objectValue = value as object;
  const existing = projectionObjectIdentity.get(objectValue);
  if (existing) return existing;
  const next = ++projectionObjectIdentityClock;
  projectionObjectIdentity.set(objectValue, next);
  return next;
};

const isMaterializedMessageAligned = (
  materialized: ChatMessageLike,
  source: ChatRuntimeMessageProjection
): boolean =>
  materialized.__runtime_projected === true &&
  materialized.__runtime_message_id === source.id &&
  materialized.__runtime_user_turn_id === source.userTurnId &&
  materialized.__runtime_model_turn_id === source.modelTurnId &&
  materialized.__runtime_render_key === resolveChatRuntimeProjectionKey(source) &&
  materialized.role === source.role &&
  materialized.content === source.content &&
  materialized.reasoning === source.reasoning &&
  materialized.runtime_status === source.status &&
  materialized.message_id === source.id;

const MATERIALIZED_MUTABLE_FIELDS = [
  'attachments',
  'feedback',
  'plan',
  'questionPanel',
  'stats',
  'subagents',
  'workflowItems'
];

const buildMaterializedMutableFieldsRevision = (message: ChatMessageLike): string => {
  const values = MATERIALIZED_MUTABLE_FIELDS
    .map((field) => [field, message[field]])
    .filter(([, value]) => value !== undefined);
  if (values.length === 0) return '';
  try {
    return JSON.stringify(values) || '';
  } catch {
    return values
      .map(([field, value]) => `${field}:${Array.isArray(value) ? value.length : typeof value}`)
      .join('|');
  }
};

const cloneDisplayProjection = (display: unknown): ChatMessageLike => {
  if (!isPlainRecord(display)) return {};
  return Object.fromEntries(
    Object.entries(display).map(([key, value]) => [key, cloneDisplayValue(value)])
  );
};

const cloneDisplayValue = (value: unknown): unknown => {
  if (Array.isArray(value)) {
    return value.map(cloneDisplayValue);
  }
  if (isPlainRecord(value)) {
    return Object.fromEntries(
      Object.entries(value).map(([key, inner]) => [key, cloneDisplayValue(inner)])
    );
  }
  return value;
};

const isSyntheticGreetingDisplay = (display: unknown): boolean =>
  isPlainRecord(display) && (display.isGreeting === true || display.is_greeting === true);

export const resolveChatRuntimeMessageRenderKey = (
  message: ChatMessageLike | null | undefined
): string => resolveChatRuntimeRenderableKey(message);

const resolveChatRuntimeProjectionKey = (
  message: ChatRuntimeMessageProjection
): string => `runtime:${message.role}:${message.id}`;

const resolveProjectedWorkflowStreaming = (
  message: ChatRuntimeMessageProjection
): boolean => {
  if (isProjectedResumablePause(message)) return false;
  return (
    message.status === 'tooling' ||
    hasActiveProjectedWorkflowItems(message.workflowItems) ||
    hasActiveProjectedSubagents(message.subagents) ||
    (isRuntimeMessageActive(message.status) && !message.content && !message.reasoning)
  );
};

const isProjectedResumablePause = (
  message: ChatRuntimeMessageProjection
): boolean => {
  const display = isPlainRecord(message.display) ? message.display : {};
  return normalizeFlag(display.resume_available ?? display.resumeAvailable) ||
    normalizeFlag(display.slow_client ?? display.slowClient);
};

const hasActiveProjectedWorkflowItems = (items: unknown): boolean => {
  if (!Array.isArray(items)) return false;
  return items.some((item) => {
    if (!isPlainRecord(item)) return false;
    const status = normalizeStatus(item.status);
    return status === 'loading' || status === 'pending' || status === 'running' || status === 'streaming';
  });
};

const hasActiveProjectedSubagents = (items: unknown): boolean => {
  if (!Array.isArray(items)) return false;
  return items.some((item) => {
    if (!isPlainRecord(item)) return false;
    const agentState = isPlainRecord(item.agent_state)
      ? item.agent_state
      : isPlainRecord(item.agentState)
        ? item.agentState
        : {};
    const status = normalizeStatus(item.status ?? agentState.status);
    if (
      status === 'accepted' ||
      status === 'in_progress' ||
      status === 'inprogress' ||
      status === 'loading' ||
      status === 'pending' ||
      status === 'processing' ||
      status === 'queued' ||
      status === 'running' ||
      status === 'started' ||
      status === 'waiting'
    ) {
      return true;
    }
    if (
      status === 'complete' ||
      status === 'completed' ||
      status === 'done' ||
      status === 'finished' ||
      status === 'idle' ||
      status === 'success' ||
      status === 'succeeded' ||
      status === 'aborted' ||
      status === 'cancelled' ||
      status === 'canceled' ||
      status === 'closed' ||
      status === 'error' ||
      status === 'failed'
    ) {
      return false;
    }
    if (normalizeFlag(item.terminal) || normalizeFlag(item.failed)) return false;
    return Boolean(firstText(item.session_id, item.sessionId, item.run_id, item.runId));
  });
};

const cloneProjectionRecords = (
  projected: unknown,
  fallback: unknown
): Record<string, unknown>[] => {
  const source = Array.isArray(projected) ? projected : Array.isArray(fallback) ? fallback : [];
  return source.filter(isPlainRecord).map((item) => ({ ...item }));
};

const resolveAssistantLegacyState = (
  status: ChatRuntimeMessageStatus
): 'running' | 'done' | 'error' => {
  if (isRuntimeMessageActive(status)) return 'running';
  if (status === 'failed' || status === 'cancelled') return 'error';
  return 'done';
};

const readRuntimeRenderRawFlag = (): string => {
  if (typeof window === 'undefined') return '';
  try {
    for (const key of RENDER_STORAGE_KEYS) {
      const raw = String(window.localStorage.getItem(key) || '')
        .trim()
        .toLowerCase();
      if (raw) return raw;
    }
  } catch {
    // Ignore storage access failures in restricted browser contexts.
  }
  try {
    const params = new URLSearchParams(window.location.search || '');
    for (const key of RENDER_SEARCH_KEYS) {
      const raw = String(params.get(key) || '')
        .trim()
        .toLowerCase();
      if (raw) return raw;
    }
  } catch {
    // Ignore invalid URL state.
  }
  return '';
};

const readRuntimeRenderNamedFlag = (keys: string[]): boolean => {
  if (typeof window === 'undefined') return false;
  try {
    for (const key of keys) {
      const raw = String(window.localStorage.getItem(key) || '')
        .trim()
        .toLowerCase();
      if (RENDER_TRUE_VALUES.has(raw) || RENDER_SHADOW_VALUES.has(raw)) {
        return true;
      }
    }
  } catch {
    // Ignore storage access failures in restricted browser contexts.
  }
  return false;
};

const readRuntimeRenderNamedSearchFlag = (keys: string[]): boolean => {
  if (typeof window === 'undefined') return false;
  try {
    const params = new URLSearchParams(window.location.search || '');
    for (const key of keys) {
      const raw = String(params.get(key) || '')
        .trim()
        .toLowerCase();
      if (RENDER_TRUE_VALUES.has(raw) || RENDER_SHADOW_VALUES.has(raw)) {
        return true;
      }
    }
  } catch {
    return false;
  }
  return false;
};

const isPlainRecord = (value: unknown): value is ChatMessageLike =>
  Boolean(value && typeof value === 'object' && !Array.isArray(value));

const firstText = (...values: unknown[]): string => {
  for (const value of values) {
    const text = String(value ?? '').trim();
    if (text) return text;
  }
  return '';
};

const normalizeStatus = (value: unknown): string =>
  String(value || '').trim().toLowerCase();

const normalizeFlag = (value: unknown): boolean => {
  if (typeof value === 'string') {
    const text = value.trim().toLowerCase();
    if (!text) return false;
    return text !== 'false' && text !== '0' && text !== 'no';
  }
  return Boolean(value);
};
