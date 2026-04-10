import {
  compareMissionChatMessages,
  type MissionChatMessage
} from './beeroomCanvasChatModel';

export const buildBeeroomRuntimeRelayMessageSignature = (
  messages: MissionChatMessage[]
): string =>
  messages
    .map((message) =>
      [
        String(message?.key || '').trim(),
        String(message?.tone || '').trim(),
        String(message?.senderName || '').trim(),
        String(message?.senderAgentId || '').trim(),
        String(message?.mention || '').trim(),
        String(message?.body || '').trim(),
        String(message?.meta || '').trim(),
        String(message?.avatarImageUrl || '').trim(),
        Number(message?.time || 0),
        Number(message?.sortOrder || 0)
      ].join(':')
    )
    .join('|');

export const mergeBeeroomRuntimeRelayMessages = (
  current: MissionChatMessage[],
  incoming: MissionChatMessage[],
  limit: number
): MissionChatMessage[] => {
  const merged = new Map<string, MissionChatMessage>();
  [...current, ...incoming].forEach((message) => {
    const key = String(message?.key || '').trim();
    if (!key) return;
    merged.set(key, message);
  });
  return Array.from(merged.values())
    .sort(compareMissionChatMessages)
    .slice(-Math.max(1, Math.floor(limit)));
};

export const filterBeeroomRuntimeRelayMessagesAfter = (
  messages: MissionChatMessage[],
  clearedAfter: number
): MissionChatMessage[] => {
  if (!clearedAfter) {
    return [...messages].sort(compareMissionChatMessages);
  }
  return messages
    .filter((message) => Number(message?.time || 0) > clearedAfter)
    .sort(compareMissionChatMessages);
};
