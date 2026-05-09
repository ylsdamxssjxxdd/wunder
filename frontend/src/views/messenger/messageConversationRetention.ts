export type MessageConversationRetentionInput = {
  activeConversationId?: unknown;
  routeConversationId?: unknown;
  routeSessionId?: unknown;
  routeAgentId?: unknown;
  routeEntry?: unknown;
  activeSessionId?: unknown;
  draftAgentId?: unknown;
  messageCount?: unknown;
  worldConversationId?: unknown;
  worldMessageCount?: unknown;
};

const normalizeText = (value: unknown): string => String(value || '').trim();

const normalizeCount = (value: unknown): number => {
  const numeric = Number(value);
  return Number.isFinite(numeric) ? Math.max(0, Math.floor(numeric)) : 0;
};

export function hasRetainedMessageConversationContext(
  input: MessageConversationRetentionInput | null | undefined
): boolean {
  if (!input) {
    return false;
  }
  if (normalizeText(input.activeConversationId)) {
    return true;
  }
  if (normalizeText(input.routeConversationId)) {
    return true;
  }
  if (normalizeText(input.routeSessionId)) {
    return true;
  }
  const routeEntry = normalizeText(input.routeEntry).toLowerCase();
  if (normalizeText(input.routeAgentId) || routeEntry === 'default') {
    return true;
  }
  if (normalizeText(input.activeSessionId)) {
    return true;
  }
  if (normalizeText(input.draftAgentId)) {
    return true;
  }
  if (normalizeCount(input.messageCount) > 0) {
    return true;
  }
  if (normalizeText(input.worldConversationId)) {
    return true;
  }
  if (normalizeCount(input.worldMessageCount) > 0) {
    return true;
  }
  return false;
}
