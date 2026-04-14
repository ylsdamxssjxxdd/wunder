import { compareMissionChatMessages, type MissionChatMessage } from '@/components/beeroom/beeroomCanvasChatModel';

export const DEFAULT_BEEROOM_AGENT_OUTPUT_PREVIEW_LIMIT = 6;

export const listRecentBeeroomAgentOutputs = (
  messages: MissionChatMessage[],
  options: {
    agentId: unknown;
    limit?: number;
  }
): MissionChatMessage[] => {
  const agentId = String(options.agentId || '').trim();
  if (!agentId) return [];
  const requestedLimit = Number(options.limit);
  const limit =
    Number.isFinite(requestedLimit) && requestedLimit > 0
      ? Math.max(1, Math.floor(requestedLimit))
      : DEFAULT_BEEROOM_AGENT_OUTPUT_PREVIEW_LIMIT;

  return (Array.isArray(messages) ? messages : [])
    .filter((message) => {
      if (!message) return false;
      if (message.tone === 'user' || message.tone === 'system') return false;
      if (String(message.senderAgentId || '').trim() !== agentId) return false;
      return String(message.body || '').trim().length > 0;
    })
    .slice()
    .sort(compareMissionChatMessages)
    .slice(-limit)
    .reverse();
};
