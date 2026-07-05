import {
  compareMissionChatMessages,
  type MissionChatMessage
} from './beeroomCanvasChatModel';

const normalizeText = (value: unknown) => String(value || '').trim();

const isSessionScopedMessageKey = (key: unknown, sessionId: string) =>
  normalizeText(key).startsWith(`session:${normalizeText(sessionId)}:`);

const isAnySessionScopedMessageKey = (key: unknown) =>
  normalizeText(key).startsWith('session:');

const areEquivalentMissionChatMessages = (
  localMessage: MissionChatMessage,
  remoteMessage: MissionChatMessage
): boolean => {
  if (localMessage.tone !== remoteMessage.tone) return false;
  if (normalizeText(localMessage.body) !== normalizeText(remoteMessage.body)) return false;
  if (normalizeText(localMessage.mention) !== normalizeText(remoteMessage.mention)) return false;
  if (normalizeText(localMessage.senderName) !== normalizeText(remoteMessage.senderName)) return false;
  if (normalizeText(localMessage.senderAgentId) !== normalizeText(remoteMessage.senderAgentId)) return false;
  return Math.abs(Number(localMessage.time || 0) - Number(remoteMessage.time || 0)) <= 8;
};

const buildStableRemoteMessage = (
  current: MissionChatMessage[],
  remoteMessage: MissionChatMessage,
  sessionId: string
): MissionChatMessage => {
  if (!isSessionScopedMessageKey(remoteMessage.key, sessionId)) return remoteMessage;
  const equivalentLocalMessage = current.find((localMessage) => {
    if (isAnySessionScopedMessageKey(localMessage.key)) return false;
    return areEquivalentMissionChatMessages(localMessage, remoteMessage);
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

  const stableIncoming = incoming.map((remoteMessage) =>
    buildStableRemoteMessage(current, remoteMessage, sessionId)
  );

  const preservedLocalMessages = current.filter((message) => {
    if (isSessionScopedMessageKey(message.key, sessionId)) {
      return false;
    }
    return !stableIncoming.some((remoteMessage) => areEquivalentMissionChatMessages(message, remoteMessage));
  });

  return [...stableIncoming, ...preservedLocalMessages]
    .sort(compareMissionChatMessages)
    .slice(-Math.max(1, Math.floor(options.limit)));
};
