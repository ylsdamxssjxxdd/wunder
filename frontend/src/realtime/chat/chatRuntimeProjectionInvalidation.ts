import { applyChatRuntimeEvent } from './chatRuntimeReducer';
import type {
  ChatRuntimeApplyResult,
  ChatRuntimeEvent,
  ChatRuntimeProjection
} from './chatRuntimeTypes';

type ProjectionVersionStore = {
  runtimeProjectionVersion?: unknown;
};

export const runtimeProjectionInvalidationState = {
  cancel: null as null | (() => void),
  pending: false
};

export const markRuntimeProjectionChanged = (
  store: ProjectionVersionStore | null | undefined,
  options: { immediate?: boolean; reason?: string } = {}
) => {
  if (!store || typeof store !== 'object') return;
  const bump = () => {
    runtimeProjectionInvalidationState.cancel = null;
    runtimeProjectionInvalidationState.pending = false;
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
  if (typeof requestAnimationFrame === 'function') {
    const frame = requestAnimationFrame(() => bump());
    runtimeProjectionInvalidationState.cancel = () => cancelAnimationFrame(frame);
    return;
  }
  const timer = globalThis.setTimeout(() => bump(), 16);
  runtimeProjectionInvalidationState.cancel = () => globalThis.clearTimeout(timer);
};

export const clearRuntimeProjectionInvalidation = () => {
  if (runtimeProjectionInvalidationState.cancel) {
    runtimeProjectionInvalidationState.cancel();
  }
  runtimeProjectionInvalidationState.cancel = null;
  runtimeProjectionInvalidationState.pending = false;
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
    markRuntimeProjectionChanged(store, options);
  }
  return results;
};
