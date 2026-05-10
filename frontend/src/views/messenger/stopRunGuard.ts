import { isAssistantRuntimeRunning } from '@/utils/chatSessionRuntime';

type ChatMessageLike = Record<string, unknown>;

export type StopRunGuardBlockReason =
  | 'not_running'
  | 'session_changed'
  | 'run_changed';

export interface StopRunSnapshot {
  sessionId: string;
  busy: boolean;
  messageCount: number;
  latestUserKey: string;
  latestUserIndex: number;
  runningAssistantKey: string;
  runningAssistantIndex: number;
}

export interface StopRunGuardDecision {
  ok: boolean;
  reason: StopRunGuardBlockReason | null;
}

const normalizeSessionId = (value: unknown): string => String(value || '').trim();

const normalizeScalar = (value: unknown): string => {
  if (value === null || value === undefined) return '';
  return String(value).trim();
};

const resolveMessageStableId = (message: ChatMessageLike): string =>
  normalizeScalar(
    message.id ??
      message.message_id ??
      message.messageId ??
      message.localId ??
      message.local_id ??
      message.client_message_id ??
      message.clientMessageId ??
      message.request_id ??
      message.requestId
  );

const buildMessageKey = (message: ChatMessageLike | null | undefined, index: number): string => {
  if (!message || index < 0) return '';
  const role = normalizeScalar(message.role);
  const stableId = resolveMessageStableId(message);
  const createdAt = normalizeScalar(message.created_at ?? message.createdAt);
  const streamRound = normalizeScalar(message.stream_round ?? message.streamRound);
  const waitingStartedAt = normalizeScalar(
    message.waiting_updated_at_ms ??
      message.waitingUpdatedAtMs ??
      (message.stats as ChatMessageLike | null | undefined)?.interaction_start_ms ??
      (message.stats as ChatMessageLike | null | undefined)?.interactionStartMs
  );
  return [role, index, stableId, createdAt, streamRound, waitingStartedAt].join('|');
};

const resolveLatestUserIndex = (messages: ChatMessageLike[]): number => {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (messages[index]?.role === 'user') {
      return index;
    }
  }
  return -1;
};

const resolveRunningAssistantIndex = (
  messages: ChatMessageLike[],
  latestUserIndex: number
): number => {
  const startIndex = latestUserIndex >= 0 ? latestUserIndex + 1 : 0;
  for (let index = messages.length - 1; index >= startIndex; index -= 1) {
    const message = messages[index];
    if (message?.role === 'assistant' && isAssistantRuntimeRunning(message)) {
      return index;
    }
  }
  return -1;
};

export const captureStopRunSnapshot = (input: {
  sessionId: unknown;
  messages: ChatMessageLike[] | null | undefined;
  busy: unknown;
}): StopRunSnapshot => {
  const messages = Array.isArray(input.messages) ? input.messages : [];
  const latestUserIndex = resolveLatestUserIndex(messages);
  const runningAssistantIndex = resolveRunningAssistantIndex(messages, latestUserIndex);
  return {
    sessionId: normalizeSessionId(input.sessionId),
    busy: Boolean(input.busy),
    messageCount: messages.length,
    latestUserKey: buildMessageKey(messages[latestUserIndex], latestUserIndex),
    latestUserIndex,
    runningAssistantKey: buildMessageKey(messages[runningAssistantIndex], runningAssistantIndex),
    runningAssistantIndex
  };
};

export const validateStopRunSnapshot = (
  expected: StopRunSnapshot,
  current: StopRunSnapshot,
  activeSessionId: unknown
): StopRunGuardDecision => {
  if (!expected.busy || !current.busy) {
    return { ok: false, reason: 'not_running' };
  }
  const normalizedActiveSessionId = normalizeSessionId(activeSessionId);
  if (
    !expected.sessionId ||
    !current.sessionId ||
    expected.sessionId !== current.sessionId ||
    normalizedActiveSessionId !== expected.sessionId
  ) {
    return { ok: false, reason: 'session_changed' };
  }
  if (expected.latestUserKey !== current.latestUserKey) {
    return { ok: false, reason: 'run_changed' };
  }
  // Chat runs are anchored to the latest user turn. A single user turn may span
  // multiple assistant/model rounds, so the assistant key can legitimately
  // change while the original stop confirmation is still valid.
  if (expected.latestUserKey || current.latestUserKey) {
    return { ok: true, reason: null };
  }
  if (expected.runningAssistantKey !== current.runningAssistantKey) {
    return { ok: false, reason: 'run_changed' };
  }
  if (!expected.runningAssistantKey && !current.runningAssistantKey) {
    return expected.messageCount === current.messageCount
      ? { ok: true, reason: null }
      : { ok: false, reason: 'run_changed' };
  }
  return { ok: true, reason: null };
};
