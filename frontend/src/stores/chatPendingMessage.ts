type ChatMessage = Record<string, any>;

const normalizeFlag = (value: unknown): boolean => {
  if (typeof value === 'string') {
    const normalized = value.trim().toLowerCase();
    if (!normalized) return false;
    return normalized !== 'false' && normalized !== '0' && normalized !== 'no';
  }
  return Boolean(value);
};

const isAssistantRuntimeMarkedRunning = (message: ChatMessage | null | undefined): boolean =>
  Boolean(
    message?.role === 'assistant' &&
    (
      normalizeFlag(message.stream_incomplete) ||
      normalizeFlag(message.workflowStreaming) ||
      normalizeFlag(message.reasoningStreaming)
    )
  );

const resolveLatestUserIndex = (messages: ChatMessage[]): number => {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    if (messages[i]?.role === 'user') {
      return i;
    }
  }
  return -1;
};

export const isPendingAssistantMessage = (message: ChatMessage | null | undefined): boolean =>
  isAssistantRuntimeMarkedRunning(message);

export const stopPendingAssistantMessage = (message: ChatMessage | null | undefined): boolean => {
  if (!isAssistantRuntimeMarkedRunning(message)) return false;
  message.stream_incomplete = false;
  message.workflowStreaming = false;
  message.reasoningStreaming = false;
  return true;
};

export const clearSupersededPendingAssistantMessages = (
  messages: ChatMessage[] | null | undefined
): boolean => {
  if (!Array.isArray(messages) || messages.length === 0) return false;
  const latestUserIndex = resolveLatestUserIndex(messages);
  if (latestUserIndex < 0) return false;
  let changed = false;
  messages.forEach((message, index) => {
    if (index > latestUserIndex || !isPendingAssistantMessage(message)) {
      return;
    }
    stopPendingAssistantMessage(message);
    changed = true;
  });
  return changed;
};

export const findPendingAssistantMessage = (
  messages: ChatMessage[] | null | undefined
): ChatMessage | null => {
  if (!Array.isArray(messages) || messages.length === 0) return null;
  const latestUserIndex = resolveLatestUserIndex(messages);
  // Only the latest user turn may own a resumable assistant response.
  for (let i = messages.length - 1; i > latestUserIndex; i -= 1) {
    const message = messages[i];
    if (message?.role !== 'assistant') continue;
    return isPendingAssistantMessage(message) ? message : null;
  }
  return null;
};
