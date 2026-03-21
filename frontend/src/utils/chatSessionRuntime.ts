import { isCompactionRunningFromWorkflowItems } from './chatCompactionWorkflow';

type ChatMessage = Record<string, unknown>;

const normalizeFlag = (value: unknown): boolean => {
  if (typeof value === 'string') {
    const text = value.trim().toLowerCase();
    if (!text) return false;
    return text !== 'false' && text !== '0' && text !== 'no';
  }
  return Boolean(value);
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
  messages: ChatMessage[] | null | undefined
): boolean => normalizeFlag(loading) || hasRunningAssistantMessage(messages);
