import { scheduleBackgroundPublication, flushBackgroundPublication, clearBackgroundPublications } from './chatBackgroundPublication';
import { applyChatRuntimeEvent } from './chatRuntimeReducer';
import type {
  ChatRuntimeApplyResult,
  ChatRuntimeEvent,
  ChatRuntimeProjection
} from './chatRuntimeTypes';
import { chatDebugLog } from '@/utils/chatDebug';
import { chatPerf } from '@/utils/chatPerf';

type ProjectionVersionStore = {
  activeSessionId?: unknown;
  foregroundChatSessionId?: string | null;
  runtimeProjectionVersionBySession?: Record<string, number>;
  runtimeProjectionContentVersion?: unknown;
  runtimeProjectionContentVersionByMessage?: Record<string, number>;
  runtimeProjectionReasoningVersion?: unknown;
  runtimeProjectionReasoningVersionByMessage?: Record<string, number>;
  runtimeProjectionVersion?: unknown;
};

export const runtimeProjectionInvalidationState = {
  cancel: null as null | (() => void),
  pending: false,
  lastBumpedAt: 0,
  sessionIds: new Set<string>()
};

export const runtimeProjectionContentInvalidationState = {
  cancel: null as null | (() => void),
  pending: false,
  lastBumpedAt: 0,
  scheduledAt: 0,
  messageIds: new Set<string>(),
  slowFlushCount: 0,
  maxSlowFlushMs: 0
};

export const runtimeProjectionReasoningInvalidationState = {
  cancel: null as null | (() => void),
  pending: false,
  lastBumpedAt: 0,
  messageIds: new Set<string>()
};

let backgroundChanges = new WeakMap<object, {
  sessions: Set<string>;
  messages: Set<string>;
  reasoningMessages: Set<string>;
}>();
const bumpSessionClock = (store: ProjectionVersionStore, sessionId: string) => {
  if (!store.runtimeProjectionVersionBySession) store.runtimeProjectionVersionBySession = {};
  store.runtimeProjectionVersionBySession[sessionId] = Number(store.runtimeProjectionVersionBySession[sessionId] || 0) + 1;
};

const DEFAULT_PROJECTION_INVALIDATION_DELAY_MS = 24;
const DEFAULT_PROJECTION_CONTENT_INVALIDATION_DELAY_MS = 24;
const DEFAULT_PROJECTION_REASONING_INVALIDATION_DELAY_MS = 150;
const STREAM_CONTENT_DEBUG_SLOW_MS = 48;

const flushRuntimeProjectionContentVersion = (store: ProjectionVersionStore) => {
  const messageIds = Array.from(runtimeProjectionContentInvalidationState.messageIds);
  const scheduledAt = runtimeProjectionContentInvalidationState.scheduledAt;
  const now = Date.now();
  runtimeProjectionContentInvalidationState.cancel = null;
  runtimeProjectionContentInvalidationState.pending = false;
  runtimeProjectionContentInvalidationState.lastBumpedAt = now;
  runtimeProjectionContentInvalidationState.scheduledAt = 0;
  runtimeProjectionContentInvalidationState.messageIds.clear();
  if (messageIds.length === 0) return;
  store.runtimeProjectionContentVersion = Number(store.runtimeProjectionContentVersion || 0) + 1;
  if (!store.runtimeProjectionContentVersionByMessage || typeof store.runtimeProjectionContentVersionByMessage !== 'object') {
    store.runtimeProjectionContentVersionByMessage = {};
  }
  for (const messageId of messageIds) {
    store.runtimeProjectionContentVersionByMessage[messageId] =
      Number(store.runtimeProjectionContentVersionByMessage[messageId] || 0) + 1;
  }
  const latencyMs = scheduledAt > 0 ? now - scheduledAt : 0;
  if (latencyMs >= STREAM_CONTENT_DEBUG_SLOW_MS) {
    runtimeProjectionContentInvalidationState.slowFlushCount += 1;
    runtimeProjectionContentInvalidationState.maxSlowFlushMs = Math.max(
      runtimeProjectionContentInvalidationState.maxSlowFlushMs,
      latencyMs
    );
    const payload = {
      latencyMs,
      messageCount: messageIds.length,
      messageIds: messageIds.slice(0, 5),
      contentVersion: Number(store.runtimeProjectionContentVersion || 0),
      slowFlushCount: runtimeProjectionContentInvalidationState.slowFlushCount,
      maxSlowFlushMs: runtimeProjectionContentInvalidationState.maxSlowFlushMs
    };
    chatDebugLog('chat.stream.perf', 'content-clock-slow-flush', payload);
    chatPerf.recordDuration('chat_stream_content_clock_slow_flush', latencyMs, payload);
  } else if (chatPerf.enabled()) {
    chatPerf.recordDuration('chat_stream_content_clock_flush', latencyMs, {
      messageCount: messageIds.length
    });
  }
};

const markRuntimeProjectionContentChanged = (
  store: ProjectionVersionStore | null | undefined,
  messageIds: Iterable<unknown>,
  options: { immediate?: boolean } = {}
) => {
  if (!store || typeof store !== 'object') return;
  for (const rawMessageId of messageIds) {
    const messageId = String(rawMessageId || '').trim();
    if (!messageId) continue;
    runtimeProjectionContentInvalidationState.messageIds.add(messageId);
  }
  if (runtimeProjectionContentInvalidationState.messageIds.size === 0) return;
  const bump = () => flushRuntimeProjectionContentVersion(store);
  if (options.immediate === true) {
    if (runtimeProjectionContentInvalidationState.cancel) {
      runtimeProjectionContentInvalidationState.cancel();
    }
    runtimeProjectionContentInvalidationState.scheduledAt = Date.now();
    bump();
    return;
  }
  if (runtimeProjectionContentInvalidationState.pending) return;
  runtimeProjectionContentInvalidationState.pending = true;
  runtimeProjectionContentInvalidationState.scheduledAt = Date.now();
  const elapsedMs = Date.now() - runtimeProjectionContentInvalidationState.lastBumpedAt;
  const delayMs = Math.max(0, DEFAULT_PROJECTION_CONTENT_INVALIDATION_DELAY_MS - elapsedMs);
  const timer = globalThis.setTimeout(() => bump(), delayMs);
  runtimeProjectionContentInvalidationState.cancel = () => globalThis.clearTimeout(timer);
};

const flushRuntimeProjectionReasoningVersion = (store: ProjectionVersionStore) => {
  const messageIds = Array.from(runtimeProjectionReasoningInvalidationState.messageIds);
  runtimeProjectionReasoningInvalidationState.cancel = null;
  runtimeProjectionReasoningInvalidationState.pending = false;
  runtimeProjectionReasoningInvalidationState.lastBumpedAt = Date.now();
  runtimeProjectionReasoningInvalidationState.messageIds.clear();
  if (messageIds.length === 0) return;
  store.runtimeProjectionReasoningVersion = Number(store.runtimeProjectionReasoningVersion || 0) + 1;
  if (
    !store.runtimeProjectionReasoningVersionByMessage ||
    typeof store.runtimeProjectionReasoningVersionByMessage !== 'object'
  ) {
    store.runtimeProjectionReasoningVersionByMessage = {};
  }
  for (const messageId of messageIds) {
    store.runtimeProjectionReasoningVersionByMessage[messageId] =
      Number(store.runtimeProjectionReasoningVersionByMessage[messageId] || 0) + 1;
  }
};

const markRuntimeProjectionReasoningChanged = (
  store: ProjectionVersionStore | null | undefined,
  messageIds: Iterable<unknown>,
  options: { immediate?: boolean } = {}
) => {
  if (!store || typeof store !== 'object') return;
  for (const rawMessageId of messageIds) {
    const messageId = String(rawMessageId || '').trim();
    if (messageId) runtimeProjectionReasoningInvalidationState.messageIds.add(messageId);
  }
  if (runtimeProjectionReasoningInvalidationState.messageIds.size === 0) return;
  const bump = () => flushRuntimeProjectionReasoningVersion(store);
  if (options.immediate === true) {
    if (runtimeProjectionReasoningInvalidationState.cancel) {
      runtimeProjectionReasoningInvalidationState.cancel();
    }
    bump();
    return;
  }
  if (runtimeProjectionReasoningInvalidationState.pending) return;
  runtimeProjectionReasoningInvalidationState.pending = true;
  const elapsedMs = Date.now() - runtimeProjectionReasoningInvalidationState.lastBumpedAt;
  const delayMs = Math.max(0, DEFAULT_PROJECTION_REASONING_INVALIDATION_DELAY_MS - elapsedMs);
  const timer = globalThis.setTimeout(bump, delayMs);
  runtimeProjectionReasoningInvalidationState.cancel = () => globalThis.clearTimeout(timer);
};

export const markRuntimeProjectionChanged = (
  store: ProjectionVersionStore | null | undefined,
  options: { immediate?: boolean; reason?: string; sessionId?: string; sessionIds?: Iterable<string> } = {}
) => {
  if (!store || typeof store !== 'object') return;
  if (options.sessionId) runtimeProjectionInvalidationState.sessionIds.add(options.sessionId);
  for (const id of options.sessionIds || []) runtimeProjectionInvalidationState.sessionIds.add(id);
  const bump = () => {
    runtimeProjectionInvalidationState.sessionIds.forEach(id => bumpSessionClock(store, id));
    runtimeProjectionInvalidationState.sessionIds.clear();
    runtimeProjectionInvalidationState.cancel = null;
    runtimeProjectionInvalidationState.pending = false;
    runtimeProjectionInvalidationState.lastBumpedAt = Date.now();
    store.runtimeProjectionVersion = Number(store.runtimeProjectionVersion || 0) + 1;
  };
  if (options.immediate === true) {
    if (runtimeProjectionInvalidationState.cancel) {
      runtimeProjectionInvalidationState.cancel();
    }
    bump();
    return;
  }
  if (runtimeProjectionInvalidationState.pending) return;
  runtimeProjectionInvalidationState.pending = true;
  const elapsedMs = Date.now() - runtimeProjectionInvalidationState.lastBumpedAt;
  const delayMs = Math.max(0, DEFAULT_PROJECTION_INVALIDATION_DELAY_MS - elapsedMs);
  if (typeof requestAnimationFrame === 'function') {
    let timer: ReturnType<typeof setTimeout> | null = null;
    let frame: number | null = null;
    let fallback: ReturnType<typeof setTimeout> | null = null;
    let flushed = false;
    const run = () => {
      if (flushed) return;
      flushed = true;
      if (timer !== null) {
        clearTimeout(timer);
        timer = null;
      }
      if (frame !== null && typeof cancelAnimationFrame === 'function') {
        cancelAnimationFrame(frame);
        frame = null;
      }
      if (fallback !== null) {
        clearTimeout(fallback);
        fallback = null;
      }
      bump();
    };
    const scheduleFrame = () => {
      frame = requestAnimationFrame(run);
      fallback = setTimeout(run, DEFAULT_PROJECTION_INVALIDATION_DELAY_MS);
    };
    if (delayMs > 0) {
      timer = setTimeout(run, delayMs);
    } else {
      scheduleFrame();
    }
    runtimeProjectionInvalidationState.cancel = () => {
      if (timer !== null) {
        clearTimeout(timer);
      }
      if (frame !== null && typeof cancelAnimationFrame === 'function') {
        cancelAnimationFrame(frame);
      }
      if (fallback !== null) {
        clearTimeout(fallback);
      }
    };
    return;
  }
  const timer = globalThis.setTimeout(() => bump(), Math.max(16, delayMs));
  runtimeProjectionInvalidationState.cancel = () => globalThis.clearTimeout(timer);
};

export const clearRuntimeProjectionInvalidation = () => {
  clearBackgroundPublications();
  backgroundChanges = new WeakMap();
  runtimeProjectionInvalidationState.sessionIds.clear();
  if (runtimeProjectionInvalidationState.cancel) {
    runtimeProjectionInvalidationState.cancel();
  }
  if (runtimeProjectionContentInvalidationState.cancel) {
    runtimeProjectionContentInvalidationState.cancel();
  }
  if (runtimeProjectionReasoningInvalidationState.cancel) {
    runtimeProjectionReasoningInvalidationState.cancel();
  }
  runtimeProjectionInvalidationState.cancel = null;
  runtimeProjectionInvalidationState.pending = false;
  runtimeProjectionContentInvalidationState.cancel = null;
  runtimeProjectionContentInvalidationState.pending = false;
  runtimeProjectionContentInvalidationState.messageIds.clear();
  runtimeProjectionContentInvalidationState.slowFlushCount = 0;
  runtimeProjectionContentInvalidationState.maxSlowFlushMs = 0;
  runtimeProjectionReasoningInvalidationState.cancel = null;
  runtimeProjectionReasoningInvalidationState.pending = false;
  runtimeProjectionReasoningInvalidationState.messageIds.clear();
};

export const applyChatRuntimeEventsWithInvalidation = (
  store: ProjectionVersionStore | null | undefined,
  projection: ChatRuntimeProjection,
  events: ChatRuntimeEvent[],
  options: { immediate?: boolean; reason?: string } = {}
): ChatRuntimeApplyResult[] => {
  let changed = false;
  const results = events.map((event) => {
    const result = applyChatRuntimeEvent(projection, event);
    if (result.applied) {
      changed = true;
    }
    return result;
  });
  if (changed) {
    const appliedResults = results.filter((result) => result.applied);
    if (store) {
      const foreground = store.foregroundChatSessionId ?? store.activeSessionId;
      const background = appliedResults.every(result => result.sessionId !== String(foreground || ''));
      const urgent = options.immediate === true || events.some(event =>
        /^(session_runtime|session_snapshot|turn_completed|turn_failed|turn_cancelled|assistant_final|approval_|user_message)/.test(event.event_type));
      if (background && !urgent) {
        let changes = backgroundChanges.get(store);
        if (!changes) {
          changes = { sessions: new Set(), messages: new Set(), reasoningMessages: new Set() };
          backgroundChanges.set(store, changes);
        }
        for (const result of appliedResults) {
          if (result.contentOnly && result.messageId) {
            if (!result.reasoningOnly) changes.messages.add(result.messageId);
            if (result.reasoningChanged) changes.reasoningMessages.add(result.messageId);
          }
          else changes.sessions.add(result.sessionId);
        }
        const publish = () => {
          const next = backgroundChanges.get(store);
          backgroundChanges.delete(store);
          if (!next) return;
          next.sessions.forEach(id => bumpSessionClock(store, id));
          if (next.sessions.size) markRuntimeProjectionChanged(store, { immediate: true });
          markRuntimeProjectionContentChanged(store, next.messages, { immediate: true });
          markRuntimeProjectionReasoningChanged(store, next.reasoningMessages, { immediate: true });
        };
        scheduleBackgroundPublication(store, publish);
        if (changes.messages.size + changes.reasoningMessages.size + changes.sessions.size > 2048) {
          flushBackgroundPublication(store);
        }
        return results;
      }
      // Navigation/terminal events publish everything pending before observers run.
      if (urgent) flushBackgroundPublication(store);
    }
    const contentOnlyResults = appliedResults.filter((result) => result.contentOnly === true);
    const reasoningChangedResults = contentOnlyResults.filter((result) => result.reasoningChanged === true);
    const visibleContentResults = contentOnlyResults.filter((result) => result.reasoningOnly !== true);
    if (contentOnlyResults.length === appliedResults.length && contentOnlyResults.length > 0) {
      markRuntimeProjectionContentChanged(
        store,
        visibleContentResults.map((result) => result.messageId),
        { immediate: options.immediate }
      );
      markRuntimeProjectionReasoningChanged(
        store,
        reasoningChangedResults.map((result) => result.messageId),
        { immediate: options.immediate }
      );
    } else {
      markRuntimeProjectionChanged(store, { ...options,
        sessionIds: new Set(appliedResults.map(result => result.sessionId)) });
      if (contentOnlyResults.length > 0) {
        markRuntimeProjectionContentChanged(
          store,
          visibleContentResults.map((result) => result.messageId),
          { immediate: options.immediate }
        );
        markRuntimeProjectionReasoningChanged(
          store,
          reasoningChangedResults.map((result) => result.messageId),
          { immediate: true }
        );
      }
    }
  }
  return results;
};
