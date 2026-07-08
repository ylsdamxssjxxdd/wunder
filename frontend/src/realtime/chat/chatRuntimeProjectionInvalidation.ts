import { applyChatRuntimeEvent } from './chatRuntimeReducer';
import type {
  ChatRuntimeApplyResult,
  ChatRuntimeEvent,
  ChatRuntimeProjection
} from './chatRuntimeTypes';
import { chatDebugLog } from '@/utils/chatDebug';
import { chatPerf } from '@/utils/chatPerf';

type ProjectionVersionStore = {
  runtimeProjectionContentVersion?: unknown;
  runtimeProjectionContentVersionByMessage?: Record<string, number>;
  runtimeProjectionVersion?: unknown;
};

export const runtimeProjectionInvalidationState = {
  cancel: null as null | (() => void),
  pending: false,
  lastBumpedAt: 0
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

const DEFAULT_PROJECTION_INVALIDATION_DELAY_MS = 24;
const DEFAULT_PROJECTION_CONTENT_INVALIDATION_DELAY_MS = 24;
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

export const markRuntimeProjectionChanged = (
  store: ProjectionVersionStore | null | undefined,
  options: { immediate?: boolean; reason?: string } = {}
) => {
  if (!store || typeof store !== 'object') return;
  const bump = () => {
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
  if (runtimeProjectionInvalidationState.cancel) {
    runtimeProjectionInvalidationState.cancel();
  }
  if (runtimeProjectionContentInvalidationState.cancel) {
    runtimeProjectionContentInvalidationState.cancel();
  }
  runtimeProjectionInvalidationState.cancel = null;
  runtimeProjectionInvalidationState.pending = false;
  runtimeProjectionContentInvalidationState.cancel = null;
  runtimeProjectionContentInvalidationState.pending = false;
  runtimeProjectionContentInvalidationState.messageIds.clear();
  runtimeProjectionContentInvalidationState.slowFlushCount = 0;
  runtimeProjectionContentInvalidationState.maxSlowFlushMs = 0;
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
    const contentOnlyResults = appliedResults.filter((result) => result.contentOnly === true);
    if (contentOnlyResults.length === appliedResults.length && contentOnlyResults.length > 0) {
      markRuntimeProjectionContentChanged(
        store,
        contentOnlyResults.map((result) => result.messageId),
        { immediate: options.immediate }
      );
    } else {
      markRuntimeProjectionChanged(store, options);
      if (contentOnlyResults.length > 0) {
        markRuntimeProjectionContentChanged(
          store,
          contentOnlyResults.map((result) => result.messageId),
          { immediate: options.immediate }
        );
      }
    }
  }
  return results;
};
