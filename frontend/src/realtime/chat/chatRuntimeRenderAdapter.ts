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

export type ChatRuntimeProjectionRenderMode = 'legacy' | 'shadow' | 'projection';

export type ChatRuntimeRenderableSourceDecision = {
  source: 'legacy' | 'projection';
  event: 'legacy-source' | 'projection-source' | 'projection-empty-fallback' | 'projection-shadow';
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
const RENDER_SEARCH_KEYS = ['chat_runtime_render', 'chatRuntimeRender'];
const RENDER_SHADOW_SEARCH_KEYS = ['chat_runtime_render_shadow', 'chatRuntimeRenderShadow'];

export const isChatRuntimeProjectionRenderEnabled = (): boolean =>
  resolveChatRuntimeProjectionRenderMode() === 'projection';

export const isChatRuntimeProjectionRenderShadowEnabled = (): boolean =>
  resolveChatRuntimeProjectionRenderMode() === 'shadow' ||
  readRuntimeRenderNamedFlag(RENDER_SHADOW_STORAGE_KEYS) ||
  readRuntimeRenderNamedSearchFlag(RENDER_SHADOW_SEARCH_KEYS);

export const resolveChatRuntimeProjectionRenderMode = (): ChatRuntimeProjectionRenderMode => {
  const raw = readRuntimeRenderRawFlag();
  if (RENDER_SHADOW_VALUES.has(raw)) return 'shadow';
  if (RENDER_TRUE_VALUES.has(raw) || raw === 'projection' || raw === 'projected') {
    return 'projection';
  }
  if (readRuntimeRenderNamedFlag(RENDER_SHADOW_STORAGE_KEYS) || readRuntimeRenderNamedSearchFlag(RENDER_SHADOW_SEARCH_KEYS)) {
    return 'shadow';
  }
  return 'legacy';
};

export const materializeChatRuntimeMessages = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown
): ChatMessageLike[] =>
  selectVisibleMessageProjections(projection, sessionId)
    .map(materializeChatRuntimeMessage)
    .filter((message): message is ChatMessageLike => Boolean(message));

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
  const projectionCount = Math.max(0, Number(input.projectionCount) || 0);
  const inspectShadow = input.renderMode !== 'legacy' || input.shadowEnabled === true;
  if (input.renderMode === 'projection') {
    if (projectionCount > 0 || input.projectionSessionKnown) {
      return {
        source: 'projection',
        event: 'projection-source',
        inspectShadow
      };
    }
    return {
      source: 'legacy',
      event: 'projection-empty-fallback',
      inspectShadow
    };
  }
  if (input.renderMode === 'shadow' && projectionCount > 0) {
    return {
      source: 'legacy',
      event: 'projection-shadow',
      inspectShadow
    };
  }
  return {
    source: 'legacy',
    event: 'legacy-source',
    inspectShadow
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
  const raw = isPlainRecord(message.raw) ? message.raw : null;
  if (raw && canReuseLegacyMessage(raw, message)) {
    return raw;
  }

  const active = isRuntimeMessageActive(message.status);
  const base: ChatMessageLike = raw ? { ...raw } : {};
  base.role = message.role;
  base.content = message.content;
  base.reasoning = message.reasoning;
  base.created_at = firstText(base.created_at, base.createdAt, message.createdAt);
  base.message_id = firstText(base.message_id, base.messageId, message.id);
  base.runtime_status = message.status;
  base.__runtime_projected = true;
  base.__runtime_message_id = message.id;
  base.__runtime_user_turn_id = message.userTurnId;
  base.__runtime_model_turn_id = message.modelTurnId;
  base.__runtime_render_key = resolveChatRuntimeProjectionKey(message);
  if (raw) {
    base.__runtime_raw_message = raw;
  }

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

export const resolveChatRuntimeMessageRenderKey = (
  message: ChatMessageLike | null | undefined
): string => resolveChatRuntimeRenderableKey(message);

const resolveChatRuntimeProjectionKey = (
  message: ChatRuntimeMessageProjection
): string => `runtime:${message.role}:${message.id}`;

const canReuseLegacyMessage = (
  raw: ChatMessageLike,
  projected: ChatRuntimeMessageProjection
): boolean => {
  if (!hasStableLegacyIdentity(raw)) return false;
  if (normalizeRole(raw.role) !== projected.role) return false;
  if (String(raw.content ?? '') !== projected.content) return false;
  if (String(raw.reasoning ?? '') !== projected.reasoning) return false;
  if (projected.role !== 'assistant') return true;

  const active = isRuntimeMessageActive(projected.status);
  if (normalizeFlag(raw.stream_incomplete) !== active) return false;
  if (normalizeFlag(raw.workflowStreaming) !== resolveProjectedWorkflowStreaming(projected)) {
    return false;
  }
  if (normalizeFlag(raw.reasoningStreaming) !== (active && Boolean(projected.reasoning))) {
    return false;
  }
  if (hasProjectionRecords(projected.workflowItems) || hasProjectionRecords(projected.subagents)) {
    return false;
  }
  if (projected.status === 'failed' && !normalizeFlag(raw.failed) && normalizeStatus(raw.status) !== 'failed') {
    return false;
  }
  if (
    projected.status === 'cancelled' &&
    !normalizeFlag(raw.cancelled) &&
    normalizeStatus(raw.status) !== 'cancelled' &&
    normalizeStatus(raw.status) !== 'canceled'
  ) {
    return false;
  }
  return true;
};

const hasStableLegacyIdentity = (message: ChatMessageLike): boolean =>
  Boolean(firstText(
    message.message_id,
    message.messageId,
    message.id,
    message.request_id,
    message.requestId
  ));

const hasProjectionRecords = (value: unknown): boolean =>
  Array.isArray(value) && value.some(isPlainRecord);

const resolveProjectedWorkflowStreaming = (
  message: ChatRuntimeMessageProjection
): boolean =>
  message.status === 'tooling' ||
  hasActiveProjectedWorkflowItems(message.workflowItems) ||
  hasActiveProjectedSubagents(message.subagents) ||
  (isRuntimeMessageActive(message.status) && !message.content && !message.reasoning);

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

const normalizeRole = (value: unknown): string => {
  const role = String(value || '').trim().toLowerCase();
  return role === 'user' || role === 'assistant' ? role : 'message';
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
