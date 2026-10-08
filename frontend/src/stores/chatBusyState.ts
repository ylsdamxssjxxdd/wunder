import type {
  ChatRuntimeProjection,
  ChatSessionRuntimeStatus
} from '@/realtime/chat/chatRuntimeTypes';
import { selectSessionRuntimeStatus } from '@/realtime/chat/chatRuntimeSelectors';
import { isChatRuntimeBusyStatus } from '@/realtime/chat/chatRuntimeReducer';
import { normalizeThreadRuntimeStatus } from '@/utils/chatSessionRuntime';

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

// Once a projection exists, it is the sole sequenced source of session status.
// Controllers and materialized message flags are transport/presentation state.
export const resolveMergedSessionRuntimeStatus = (
  options: ResolveMergedSessionRuntimeStatusOptions
): ChatSessionRuntimeStatus | string => {
  const sessionId = normalizeSessionId(options.sessionId);
  if (!sessionId) return 'not_loaded';
  const status = selectSessionRuntimeStatus(options.projection, sessionId);
  if (status !== 'not_loaded') return status;
  const runtimeStatus = normalizeThreadRuntimeStatus(options.runtimeStatus);
  if ((runtimeStatus === 'idle' || runtimeStatus === 'not_loaded') &&
      (options.loading || options.runtimeHasControllers)) return 'running';
  if (runtimeStatus !== 'not_loaded') return runtimeStatus;
  return options.loading || options.runtimeHasControllers ? 'running' : 'not_loaded';
};

export const resolveMergedSessionBusy = (
  options: ResolveMergedSessionBusyOptions
): boolean => {
  const status = resolveMergedSessionRuntimeStatus(options);
  // Queued work can still accept another message; running/waiting work exposes stop.
  return status !== 'queued' && isChatRuntimeBusyStatus(status);
};
