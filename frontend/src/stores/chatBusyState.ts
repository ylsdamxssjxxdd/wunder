import type {
  ChatRuntimeProjection,
  ChatSessionRuntimeStatus
} from '@/realtime/chat/chatRuntimeTypes';
import {
  selectSessionBusy,
  selectSessionRuntimeStatus
} from '@/realtime/chat/chatRuntimeSelectors';
import { isChatRuntimeBusyStatus } from '@/realtime/chat/chatRuntimeReducer';
import {
  hasActiveSubagentsAfterLatestUser,
  isSessionBusyFromSignals,
  isThreadRuntimeBusy,
  normalizeThreadRuntimeStatus
} from '@/utils/chatSessionRuntime';

type ChatMessageLike = Record<string, unknown>;

type ResolveMergedSessionBusyOptions = {
  projection?: ChatRuntimeProjection | null;
  sessionId: unknown;
  loading?: unknown;
  messages?: ChatMessageLike[] | null;
  runtimeStatus?: unknown;
  runtimeKnown?: boolean;
  runtimeHasControllers?: boolean;
};

type ResolveMergedSessionRuntimeStatusOptions = {
  projection?: ChatRuntimeProjection | null;
  sessionId: unknown;
  loading?: unknown;
  messages?: ChatMessageLike[] | null;
  runtimeStatus?: unknown;
  runtimeKnown?: boolean;
  runtimeHasControllers?: boolean;
};

const normalizeSessionId = (value: unknown): string => String(value || '').trim();

const isTerminalThreadRuntimeStatus = (value: unknown): boolean => {
  const normalized = normalizeThreadRuntimeStatus(value);
  return normalized !== 'running' && !isThreadRuntimeBusy(normalized);
};

export const resolveMergedSessionRuntimeStatus = (
  options: ResolveMergedSessionRuntimeStatusOptions
): ChatSessionRuntimeStatus | string => {
  const sessionId = normalizeSessionId(options.sessionId);
  if (!sessionId) return 'not_loaded';
  const runtimeStatus = normalizeThreadRuntimeStatus(options.runtimeStatus);
  const messages = Array.isArray(options.messages) ? options.messages : [];
  const busy = resolveMergedSessionBusy({
    projection: options.projection,
    sessionId,
    loading: options.loading,
    messages,
    runtimeStatus: options.runtimeStatus,
    runtimeKnown: options.runtimeKnown,
    runtimeHasControllers: options.runtimeHasControllers
  });
  const projectionStatus = selectSessionRuntimeStatus(
    options.projection,
    sessionId
  );
  if (isThreadRuntimeBusy(runtimeStatus)) {
    return runtimeStatus;
  }
  if (isChatRuntimeBusyStatus(projectionStatus) && busy) {
    return projectionStatus;
  }
  if (busy) {
    return 'running';
  }
  if (
    options.runtimeKnown === true &&
    options.runtimeHasControllers !== true &&
    isTerminalThreadRuntimeStatus(options.runtimeStatus)
  ) {
    return runtimeStatus;
  }
  if (projectionStatus !== 'not_loaded') {
    return projectionStatus;
  }
  return runtimeStatus;
};

export const resolveMergedSessionBusy = (
  options: ResolveMergedSessionBusyOptions
): boolean => {
  const sessionId = normalizeSessionId(options.sessionId);
  if (!sessionId) return false;
  const messages = Array.isArray(options.messages) ? options.messages : [];
  const projectionBusy = selectSessionBusy(options.projection, sessionId);
  const busyBySignals = isSessionBusyFromSignals(
    options.loading,
    messages,
    options.runtimeStatus
  );
  if (options.runtimeHasControllers === true) {
    return true;
  }
  if (!projectionBusy && !busyBySignals) {
    return false;
  }
  const hasLoadingFlag = Boolean(options.loading);
  const terminalRuntime =
    options.runtimeKnown === true &&
    isTerminalThreadRuntimeStatus(options.runtimeStatus);
  if (!hasLoadingFlag && terminalRuntime && !hasActiveSubagentsAfterLatestUser(messages)) {
    // Hydrated snapshots can briefly keep old assistant streaming flags after the
    // thread has already settled. Do not let stale projection/message flags
    // revive the composer after the runtime has confirmed an idle thread.
    return false;
  }
  return true;
};
