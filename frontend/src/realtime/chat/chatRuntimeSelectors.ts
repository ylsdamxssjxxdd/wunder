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
  const turnOrder = buildMessageTurnOrder(session);
  return ordered.sort((left, right) => {
    const leftTurnIndex = resolveMessageTurnIndex(turnOrder, left);
    const rightTurnIndex = resolveMessageTurnIndex(turnOrder, right);
    if (leftTurnIndex !== rightTurnIndex) return leftTurnIndex - rightTurnIndex;
    if (left.role !== right.role) {
      return left.role === 'user' ? -1 : 1;
    }
    const leftTime = resolveMessageCreatedAtMs(left);
    const rightTime = resolveMessageCreatedAtMs(right);
    if (leftTime !== null && rightTime !== null && leftTime !== rightTime) {
      return leftTime - rightTime;
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
  turnOrder: Map<string, number>,
  message: ChatRuntimeMessageProjection
): number => {
  return turnOrder.get(message.userTurnId) ?? Number.MAX_SAFE_INTEGER;
};

const buildMessageTurnOrder = (
  session: ChatRuntimeSessionProjection
): Map<string, number> => {
  const orderedTurns = session.userTurns
    .map((turnId, index) => ({
      turnId,
      index,
      createdAtMs: resolveTurnCreatedAtMs(session, turnId),
      createdSeq: session.userTurnById[turnId]?.createdSeq ?? Number.MAX_SAFE_INTEGER
    }))
    .sort((left, right) =>
      compareNullableTime(left.createdAtMs, right.createdAtMs) ||
      left.createdSeq - right.createdSeq || left.index - right.index
    );
  return new Map(orderedTurns.map((item, index) => [item.turnId, index]));
};

const resolveTurnCreatedAtMs = (
  session: ChatRuntimeSessionProjection,
  turnId: string
): number | null => {
  const turn = session.userTurnById[turnId];
  const directMessage = turn?.messageIds
    ?.map((messageId) => session.messageById[messageId])
    .find(Boolean);
  if (directMessage) return resolveMessageCreatedAtMs(directMessage);
  const modelMessage = turn?.modelTurnIds
    ?.flatMap((modelTurnId) => session.modelTurnById[modelTurnId]?.messageIds || [])
    .map((messageId) => session.messageById[messageId])
    .find(Boolean);
  return modelMessage ? resolveMessageCreatedAtMs(modelMessage) : null;
};

const compareNullableTime = (left: number | null, right: number | null): number => {
  if (left !== null && right !== null && left !== right) return left - right;
  if (left !== null && right === null) return -1;
  if (left === null && right !== null) return 1;
  return 0;
};

const resolveMessageCreatedAtMs = (
  message: ChatRuntimeMessageProjection
): number | null => {
  const value = Date.parse(message.createdAt || '');
  return Number.isFinite(value) ? value : null;
};
