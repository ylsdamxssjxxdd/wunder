import { hasAssistantWaitingForCurrentOutput } from '@/utils/assistantMessageRuntime';

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
      normalizeFlag(message.reasoningStreaming) ||
      hasAssistantWaitingForCurrentOutput(message)
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

const ensureAssistantInteractionEnded = (message: ChatMessage): void => {
  const waitingUpdatedAtMs = Number(
    message?.waiting_updated_at_ms ??
      message?.waitingUpdatedAtMs ??
      message?.stats?.interaction_start_ms ??
      message?.stats?.interactionStartMs ??
      0
  );
  const terminalAtMs = Number.isFinite(waitingUpdatedAtMs) && waitingUpdatedAtMs > 0
    ? Math.max(waitingUpdatedAtMs, Date.now())
    : Date.now();
  if (!message.stats || typeof message.stats !== 'object') {
    message.stats = {};
  }
  if (!Number.isFinite(Number(message.stats.interaction_start_ms)) && Number.isFinite(waitingUpdatedAtMs) && waitingUpdatedAtMs > 0) {
    message.stats.interaction_start_ms = waitingUpdatedAtMs;
  }
  const currentEndMs = Number(
    message.stats.interaction_end_ms ??
      message.stats.interactionEndMs ??
      message.stats.interaction_end ??
      message.stats.ended_at ??
      0
  );
  message.stats.interaction_end_ms =
    Number.isFinite(currentEndMs) && currentEndMs > 0
      ? Math.max(currentEndMs, terminalAtMs)
      : terminalAtMs;
};

export const stopPendingAssistantMessage = (message: ChatMessage | null | undefined): boolean => {
  if (!isAssistantRuntimeMarkedRunning(message)) return false;
  message.stream_incomplete = false;
  message.workflowStreaming = false;
  message.reasoningStreaming = false;
  if (hasAssistantWaitingForCurrentOutput(message)) {
    ensureAssistantInteractionEnded(message);
  }
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
    if (isPendingAssistantMessage(message)) {
      return message;
    }
  }
  return null;
};

export const clearTrailingPendingAssistantMessages = (
  messages: ChatMessage[] | null | undefined
): number => {
  if (!Array.isArray(messages) || messages.length === 0) return 0;
  const latestUserIndex = resolveLatestUserIndex(messages);
  let clearedCount = 0;
  for (let i = latestUserIndex + 1; i < messages.length; i += 1) {
    const message = messages[i];
    if (message?.role !== 'assistant') continue;
    if (stopPendingAssistantMessage(message)) {
      clearedCount += 1;
    }
  }
  return clearedCount;
};
