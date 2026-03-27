import { isCompactionRunningFromWorkflowItems } from './chatCompactionWorkflow';

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

export const isAssistantRuntimeRunning = (message: ChatMessage | null | undefined): boolean => {
  if (!message || message.role !== 'assistant') return false;
  if (
    normalizeFlag(message.stream_incomplete)
    || normalizeFlag(message.workflowStreaming)
    || normalizeFlag(message.reasoningStreaming)
  ) {
    return true;
  }
  return isCompactionRunningFromWorkflowItems(message.workflowItems);
};

export const hasRunningAssistantMessage = (
  messages: ChatMessage[] | null | undefined
): boolean => {
  if (!Array.isArray(messages) || messages.length === 0) return false;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
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
