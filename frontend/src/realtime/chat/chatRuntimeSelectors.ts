import type {
  ChatRuntimeMessageProjection,
  ChatRuntimeMessageStatus,
  ChatRuntimeProjection,
  ChatRuntimeSessionProjection,
  ChatSessionRuntimeStatus
} from './chatRuntimeTypes';
import { isChatRuntimeBusyStatus, normalizeChatRuntimeStatus } from './chatRuntimeReducer';

type ChatMessageLike = Record<string, unknown>;

export const selectChatRuntimeSession = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown
): ChatRuntimeSessionProjection | null => {
  const key = String(sessionId ?? '').trim();
  if (!key) return null;
  return projection?.sessions?.[key] || null;
};

export const selectSessionBusy = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown
): boolean => {
  const session = selectChatRuntimeSession(projection, sessionId);
  if (!session) return false;
  return Boolean(session.busyReason) && isChatRuntimeBusyStatus(session.runtimeStatus);
};

export const selectSessionBusyReason = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown
) => selectChatRuntimeSession(projection, sessionId)?.busyReason ?? null;

export const selectSessionRuntimeStatus = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown
): ChatSessionRuntimeStatus => {
  const session = selectChatRuntimeSession(projection, sessionId);
  return normalizeChatRuntimeStatus(session?.runtimeStatus);
};

export const selectCanSend = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown
): boolean => !selectSessionBusy(projection, sessionId);

export const selectVisibleMessageProjections = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown
): ChatRuntimeMessageProjection[] => {
  const session = selectChatRuntimeSession(projection, sessionId);
  if (!session) return [];
  const ordered: ChatRuntimeMessageProjection[] = [];
  const seen = new Set<string>();
  const pushMessage = (messageId: string): void => {
    if (!messageId || seen.has(messageId)) return;
    const message = session.messageById[messageId];
    if (!message) return;
    ordered.push(message);
    seen.add(messageId);
  };
  session.userTurns.forEach((turnId) => {
    const userTurn = session.userTurnById[turnId];
    if (!userTurn) return;
    userTurn.messageIds.forEach(pushMessage);
    userTurn.modelTurnIds.forEach((modelTurnId) => {
      const modelTurn = session.modelTurnById[modelTurnId];
      modelTurn?.messageIds?.forEach(pushMessage);
    });
  });
  session.messages.forEach(pushMessage);
  return ordered.sort((left, right) => {
    const leftTurnIndex = resolveMessageTurnIndex(session, left);
    const rightTurnIndex = resolveMessageTurnIndex(session, right);
    if (leftTurnIndex !== rightTurnIndex) return leftTurnIndex - rightTurnIndex;
    if (left.role !== right.role) {
      return left.role === 'user' ? -1 : 1;
    }
    return left.createdSeq - right.createdSeq;
  });
};

export const selectLatestAssistantForTurn = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown,
  userTurnId: unknown
): ChatRuntimeMessageProjection | null => {
  const session = selectChatRuntimeSession(projection, sessionId);
  const turnId = String(userTurnId ?? '').trim();
  if (!session || !turnId) return null;
  const userTurn = session.userTurnById[turnId];
  if (!userTurn) return null;
  for (let turnIndex = userTurn.modelTurnIds.length - 1; turnIndex >= 0; turnIndex -= 1) {
    const modelTurn = session.modelTurnById[userTurn.modelTurnIds[turnIndex]];
    if (!modelTurn) continue;
    for (let messageIndex = modelTurn.messageIds.length - 1; messageIndex >= 0; messageIndex -= 1) {
      const message = session.messageById[modelTurn.messageIds[messageIndex]];
      if (message?.role === 'assistant') return message;
    }
  }
  return null;
};

export const selectMessageRuntime = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown,
  messageId: unknown
): ChatRuntimeMessageProjection | null => {
  const session = selectChatRuntimeSession(projection, sessionId);
  const key = String(messageId ?? '').trim();
  if (!session || !key) return null;
  return session.messageById[key] || null;
};

export const selectLegacyMessageRuntime = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown,
  message: ChatMessageLike | null | undefined
): ChatRuntimeMessageProjection | null => {
  if (!message) return null;
  const session = selectChatRuntimeSession(projection, sessionId);
  if (!session) return null;
  const explicit = String(message.message_id ?? message.messageId ?? message.id ?? '').trim();
  if (explicit && session.messageById[explicit]) {
    return session.messageById[explicit];
  }
  const eventId = Number.parseInt(String(message.stream_event_id ?? message.streamEventId ?? ''), 10);
  if (Number.isFinite(eventId) && eventId > 0) {
    const byEvent = session.messageById[`legacy-event:${eventId}`];
    if (byEvent) return byEvent;
  }
  return Object.values(session.messageById).find((item) => item.raw === message) || null;
};

export const selectLegacyMessageStatus = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown,
  message: ChatMessageLike | null | undefined
): ChatRuntimeMessageStatus | null =>
  selectLegacyMessageRuntime(projection, sessionId, message)?.status ?? null;

export const isRuntimeMessageActive = (status: ChatRuntimeMessageStatus | null | undefined): boolean =>
  status === 'placeholder' ||
  status === 'waiting_first_output' ||
  status === 'streaming' ||
  status === 'tooling';

const resolveMessageTurnIndex = (
  session: ChatRuntimeSessionProjection,
  message: ChatRuntimeMessageProjection
): number => {
  const index = session.userTurns.indexOf(message.userTurnId);
  return index >= 0 ? index : Number.MAX_SAFE_INTEGER;
};
