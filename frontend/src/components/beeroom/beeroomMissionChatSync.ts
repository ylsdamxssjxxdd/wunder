import {
  compareMissionChatMessages,
  type MissionChatMessage
} from './beeroomCanvasChatModel';

const normalizeText = (value: unknown) => String(value || '').trim();

const isSessionScopedMessageKey = (key: unknown, sessionId: string) =>
  normalizeText(key).startsWith(`session:${normalizeText(sessionId)}:`);

const isDurableSessionMessageKey = (key: unknown, sessionId: string) =>
  isSessionScopedMessageKey(key, sessionId) && normalizeText(key).includes(':history:');

const preferCanonicalSessionMessage = (
  current: MissionChatMessage,
  incoming: MissionChatMessage,
  sessionId: string
): MissionChatMessage => {
  const currentIsDurable = isDurableSessionMessageKey(current.key, sessionId);
  const incomingIsDurable = isDurableSessionMessageKey(incoming.key, sessionId);
  if (currentIsDurable !== incomingIsDurable) {
    return incomingIsDurable ? incoming : current;
  }
  // Keep the first observed key when both versions have the same durability.
  return current;
};

const dedupeIncomingUserTurns = (
  messages: MissionChatMessage[],
  sessionId: string
): MissionChatMessage[] => {
  const byUserTurn = new Map<string, number>();
  const bySubmission = new Map<string, number>();
  const deduped: MissionChatMessage[] = [];
  messages.forEach((message) => {
    const userTurnId = normalizeText(message.userTurnId);
    if (message.tone !== 'user') {
      deduped.push(message);
      return;
    }
    const clientMessageId = normalizeText(message.clientMessageId);
    const exactSubmission = [
      normalizeText(message.sessionId) || sessionId,
      Number(message.time || 0).toFixed(3),
      normalizeText(message.body),
      normalizeText(message.senderName)
    ].join('|');
    const submissionIdentity = clientMessageId ? `client:${clientMessageId}` : `exact:${exactSubmission}`;
    const existingIndex =
      (userTurnId ? byUserTurn.get(userTurnId) : undefined) ?? bySubmission.get(submissionIdentity);
    if (existingIndex === undefined) {
      if (userTurnId) byUserTurn.set(userTurnId, deduped.length);
      bySubmission.set(submissionIdentity, deduped.length);
      deduped.push(message);
      return;
    }
    const preferred = preferCanonicalSessionMessage(
      deduped[existingIndex],
      message,
      sessionId
    );
    deduped[existingIndex] = preferred;
    if (userTurnId) byUserTurn.set(userTurnId, existingIndex);
    const preferredClientMessageId = normalizeText(preferred.clientMessageId);
    if (preferredClientMessageId) bySubmission.set(`client:${preferredClientMessageId}`, existingIndex);
    bySubmission.set(submissionIdentity, existingIndex);
  });
  return deduped;
};

const areEquivalentMissionChatMessages = (
  localMessage: MissionChatMessage,
  remoteMessage: MissionChatMessage
): boolean => {
  if (localMessage.tone !== remoteMessage.tone) return false;
  const localUserTurnId = normalizeText(localMessage.userTurnId);
  const remoteUserTurnId = normalizeText(remoteMessage.userTurnId);
  if (localUserTurnId && remoteUserTurnId && localUserTurnId !== remoteUserTurnId) return false;
  if (normalizeText(localMessage.body) !== normalizeText(remoteMessage.body)) return false;
  if (normalizeText(localMessage.mention) !== normalizeText(remoteMessage.mention)) return false;
  if (normalizeText(localMessage.senderName) !== normalizeText(remoteMessage.senderName)) return false;
  if (normalizeText(localMessage.senderAgentId) !== normalizeText(remoteMessage.senderAgentId)) return false;
  return Math.abs(Number(localMessage.time || 0) - Number(remoteMessage.time || 0)) <= 8;
};

// Runtime projections are append-only views but can temporarily omit earlier
// turns while an assistant delta is being reduced. Match logical turns before
// deciding whether an incoming snapshot supersedes a cached render entry.
const representsSameMissionChatTurn = (
  localMessage: MissionChatMessage,
  remoteMessage: MissionChatMessage
): boolean => {
  if (localMessage.tone !== remoteMessage.tone) return false;
  const localKey = normalizeText(localMessage.key);
  const localRemoteKey = normalizeText(localMessage.remoteKey);
  const remoteKey = normalizeText(remoteMessage.key);
  const remoteRemoteKey = normalizeText(remoteMessage.remoteKey);
  if (
    localKey === remoteKey ||
    (localRemoteKey && localRemoteKey === remoteKey) ||
    (remoteRemoteKey && remoteRemoteKey === localKey)
  ) {
    return true;
  }
  const localClientMessageId = normalizeText(localMessage.clientMessageId);
  const remoteClientMessageId = normalizeText(remoteMessage.clientMessageId);
  if (localClientMessageId && localClientMessageId === remoteClientMessageId) {
    return true;
  }
  const localUserTurnId = normalizeText(localMessage.userTurnId);
  const remoteUserTurnId = normalizeText(remoteMessage.userTurnId);
  if (localMessage.tone === 'user' && localUserTurnId && localUserTurnId === remoteUserTurnId) {
    return true;
  }
  const localModelTurnId = normalizeText(localMessage.modelTurnId);
  const remoteModelTurnId = normalizeText(remoteMessage.modelTurnId);
  if (localModelTurnId && localModelTurnId === remoteModelTurnId) {
    return true;
  }
  return areEquivalentMissionChatMessages(localMessage, remoteMessage);
};

const buildStableRemoteMessage = (
  current: MissionChatMessage[],
  remoteMessage: MissionChatMessage,
  sessionId: string
): MissionChatMessage => {
  if (!isSessionScopedMessageKey(remoteMessage.key, sessionId)) return remoteMessage;
  const equivalentLocalMessage = current.find((localMessage) => {
    const localSessionId = normalizeText(localMessage.sessionId);
    if (localSessionId && localSessionId !== sessionId) return false;
    return representsSameMissionChatTurn(localMessage, remoteMessage);
  });
  if (!equivalentLocalMessage) return remoteMessage;
  return {
    ...remoteMessage,
    key: equivalentLocalMessage.key,
    remoteKey: remoteMessage.key
  };
};

export const reconcileBeeroomSessionBackedManualMessages = (options: {
  current: MissionChatMessage[];
  incoming: MissionChatMessage[];
  sessionId: string;
  limit: number;
}): MissionChatMessage[] => {
  const sessionId = normalizeText(options.sessionId);
  const incoming = Array.isArray(options.incoming) ? options.incoming : [];
  const current = Array.isArray(options.current) ? options.current : [];
  if (!sessionId) {
    return [...incoming].sort(compareMissionChatMessages).slice(-Math.max(1, Math.floor(options.limit)));
  }

  // A history reload and the runtime projection can surface the same durable user
  // turn with different message keys. The user-turn identity is authoritative.
  const stableIncoming = dedupeIncomingUserTurns(
    incoming.map((remoteMessage) => buildStableRemoteMessage(current, remoteMessage, sessionId)),
    sessionId
  );

  const preservedLocalMessages = current.filter((message) => {
    const messageSessionId = normalizeText(message.sessionId);
    if (messageSessionId && messageSessionId !== sessionId) {
      return false;
    }
    // Session projections can be partial while a streaming event is reduced.
    // Keep any unmatched observed turn until a matching newer representation
    // arrives; a same-session conversation never removes individual turns.
    if (isSessionScopedMessageKey(message.key, sessionId)) {
      return !stableIncoming.some((remoteMessage) => representsSameMissionChatTurn(message, remoteMessage));
    }
    if (messageSessionId === sessionId) {
      return !stableIncoming.some((remoteMessage) => representsSameMissionChatTurn(message, remoteMessage));
    }
    return !stableIncoming.some((remoteMessage) => representsSameMissionChatTurn(message, remoteMessage));
  });

  return [...stableIncoming, ...preservedLocalMessages]
    .sort(compareMissionChatMessages)
    .slice(-Math.max(1, Math.floor(options.limit)));
};
