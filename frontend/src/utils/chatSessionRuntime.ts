import { isAssistantMessageRunning } from './assistantMessageRuntime';

type ChatMessage = Record<string, unknown>;
export type ThreadRuntimeStatus =
  | 'not_loaded'
  | 'idle'
  | 'running'
  | 'waiting_approval'
  | 'waiting_user_input'
  | 'system_error';

const normalizeFlag = (value: unknown): boolean => {
  if (typeof value === 'string') {
    const text = value.trim().toLowerCase();
    if (!text) return false;
    return text !== 'false' && text !== '0' && text !== 'no';
  }
  return Boolean(value);
};

const resolveLatestUserIndex = (messages: ChatMessage[]): number => {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (messages[index]?.role === 'user') {
      return index;
    }
  }
  return -1;
};

export const normalizeThreadRuntimeStatus = (value: unknown): ThreadRuntimeStatus => {
  const normalized = String(value || '').trim().toLowerCase();
  if (normalized === 'idle') return 'idle';
  if (normalized === 'running') return 'running';
  if (normalized === 'waiting_approval') return 'waiting_approval';
  if (normalized === 'waiting_user_input') return 'waiting_user_input';
  if (normalized === 'system_error') return 'system_error';
  return 'not_loaded';
};

export const isThreadRuntimeWaiting = (status: unknown): boolean => {
  const normalized = normalizeThreadRuntimeStatus(status);
  return normalized === 'waiting_approval' || normalized === 'waiting_user_input';
};

export const isThreadRuntimeBusy = (status: unknown): boolean => {
  const normalized = normalizeThreadRuntimeStatus(status);
  return normalized === 'running' || isThreadRuntimeWaiting(normalized);
};

export const didThreadRuntimeEnterBusyState = (
  previousStatus: unknown,
  nextStatus: unknown
): boolean => {
  const previous = normalizeThreadRuntimeStatus(previousStatus);
  const next = normalizeThreadRuntimeStatus(nextStatus);
  return previous !== next && !isThreadRuntimeBusy(previous) && isThreadRuntimeBusy(next);
};

export const isAssistantRuntimeRunning = (message: ChatMessage | null | undefined): boolean => {
  return isAssistantMessageRunning(message);
};

export const hasRunningAssistantMessage = (
  messages: ChatMessage[] | null | undefined
): boolean => {
  if (!Array.isArray(messages) || messages.length === 0) return false;
  const latestUserIndex = resolveLatestUserIndex(messages);
  const startIndex = latestUserIndex >= 0 ? latestUserIndex + 1 : 0;
  for (let index = messages.length - 1; index >= startIndex; index -= 1) {
    if (isAssistantRuntimeRunning(messages[index])) {
      return true;
    }
  }
  return false;
};

export const isSessionBusyFromSignals = (
  loading: unknown,
  messages: ChatMessage[] | null | undefined,
  threadStatus: unknown = null
): boolean => normalizeFlag(loading) || isThreadRuntimeBusy(threadStatus) || hasRunningAssistantMessage(messages);
