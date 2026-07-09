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

export const selectChatRuntimeMessage = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown,
  messageId: unknown
): ChatRuntimeMessageProjection | null => {
  const session = selectChatRuntimeSession(projection, sessionId);
  const key = String(messageId ?? '').trim();
  if (!session || !key) return null;
  return session.messageById[key] || null;
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

export const selectRuntimeLastAppliedEventId = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown
): number => {
  const session = selectChatRuntimeSession(projection, sessionId);
  if (!session) return 0;
  const direct = normalizePositiveEventId(session.lastAppliedEventId);
  if (direct > 0) return direct;
  return Object.keys(session.eventIdIndex || {}).reduce((maxEventId, eventId) => {
    const normalized = normalizePositiveEventId(eventId);
    return normalized > maxEventId ? normalized : maxEventId;
  }, 0);
};

const normalizePositiveEventId = (value: unknown): number => {
  const text = String(value ?? '').trim();
  if (!/^\d+$/.test(text)) return 0;
  const parsed = Number.parseInt(text, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
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
    if (isSyntheticGreetingProjection(message)) return;
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
    const leftTurnOrder = resolveMessageTurnOrder(turnOrder, left);
    const rightTurnOrder = resolveMessageTurnOrder(turnOrder, right);
    if (leftTurnOrder !== rightTurnOrder) {
      return leftTurnOrder - rightTurnOrder;
    }
    if (left.role !== right.role) {
      return left.role === 'user' ? -1 : 1;
    }
    if (left.createdSeq !== right.createdSeq) return left.createdSeq - right.createdSeq;
    return left.id.localeCompare(right.id);
  });
};

const isSyntheticGreetingProjection = (
  message: ChatRuntimeMessageProjection | null | undefined
): boolean => Boolean(message?.display?.isGreeting === true || message?.display?.is_greeting === true);

const buildMessageTurnOrder = (
  session: ChatRuntimeSessionProjection
): Map<string, number> => {
  const orderedTurns = session.userTurns
    .map((turnId, index) => ({
      turnId,
      index,
      semanticOrder: resolveTurnSemanticOrder(turnId),
      createdSeq: resolveTurnCreatedSeq(session, turnId)
    }))
    .sort((left, right) => {
      if (
        left.semanticOrder !== null &&
        right.semanticOrder !== null &&
        left.semanticOrder !== right.semanticOrder
      ) {
        return left.semanticOrder - right.semanticOrder;
      }
      return left.createdSeq - right.createdSeq || left.index - right.index;
    });
  return new Map(orderedTurns.map((item, index) => [item.turnId, index]));
};

const resolveMessageTurnOrder = (
  turnOrder: Map<string, number>,
  message: ChatRuntimeMessageProjection
): number =>
  turnOrder.get(message.userTurnId) ?? Number.MAX_SAFE_INTEGER;

const resolveTurnCreatedSeq = (
  session: ChatRuntimeSessionProjection,
  turnId: string
): number => {
  const turn = session.userTurnById[turnId];
  if (!turn) return Number.MAX_SAFE_INTEGER;
  const seqs = [
    turn.createdSeq,
    ...turn.messageIds.map((messageId) => session.messageById[messageId]?.createdSeq),
    ...turn.modelTurnIds.flatMap((modelTurnId) => {
      const modelTurn = session.modelTurnById[modelTurnId];
      return [
        modelTurn?.createdSeq,
        ...(modelTurn?.messageIds || []).map((messageId) => session.messageById[messageId]?.createdSeq)
      ];
    })
  ].filter((value): value is number => Number.isFinite(value));
  return seqs.length > 0 ? Math.min(...seqs) : Number.MAX_SAFE_INTEGER;
};

const resolveTurnSemanticOrder = (turnId: string): number | null => {
  const text = String(turnId || '').trim();
  if (!text) return null;
  const roundMatch = text.match(/(?:^|:)round:(\d+)(?::|$)/i);
  if (roundMatch) {
    return normalizePositiveInteger(roundMatch[1]);
  }
  const userMatch = text.match(/(?:^|:)user:(\d+)(?::|$)/i);
  if (userMatch) {
    return normalizePositiveInteger(userMatch[1]);
  }
  return null;
};

const normalizePositiveInteger = (value: unknown): number | null => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
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
  status === 'queued' ||
  status === 'waiting_first_output' ||
  status === 'streaming' ||
  status === 'tooling';
