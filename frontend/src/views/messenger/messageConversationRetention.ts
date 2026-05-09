export type MessageConversationRetentionInput = {
  foregroundLock?: unknown;
  activeConversationKind?: unknown;
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

export type MessageConversationKind = 'agent' | 'world' | '';

const normalizeText = (value: unknown): string => String(value || '').trim();
const normalizeKind = (value: unknown): string => String(value || '').trim().toLowerCase();

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
  if (input.foregroundLock === true) {
    return true;
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

export function hasRetainedAgentConversationContext(
  input: MessageConversationRetentionInput | null | undefined
): boolean {
  if (!input) {
    return false;
  }
  if (input.foregroundLock === true) {
    return true;
  }
  const activeConversationKind = normalizeKind(input.activeConversationKind);
  if (activeConversationKind === 'direct' || activeConversationKind === 'group') {
    return false;
  }
  if (activeConversationKind === 'agent' && normalizeText(input.activeConversationId)) {
    return true;
  }
  if (normalizeText(input.routeConversationId)) {
    return false;
  }
  if (normalizeText(input.worldConversationId)) {
    return false;
  }
  if (normalizeCount(input.worldMessageCount) > 0) {
    return false;
  }
  const routeEntry = normalizeText(input.routeEntry).toLowerCase();
  if (normalizeText(input.routeSessionId)) {
    return true;
  }
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
  return false;
}

export function resolveMessageConversationKind(
  input: MessageConversationRetentionInput | null | undefined
): MessageConversationKind {
  if (!input) {
    return '';
  }
  const activeConversationKind = normalizeKind(input.activeConversationKind);
  if (activeConversationKind === 'direct' || activeConversationKind === 'group') {
    return 'world';
  }
  if (normalizeText(input.routeConversationId)) {
    return 'world';
  }
  const routeSessionId = normalizeText(input.routeSessionId);
  const routeAgentId = normalizeText(input.routeAgentId);
  const routeEntry = normalizeText(input.routeEntry).toLowerCase();
  if (routeSessionId || routeAgentId || routeEntry === 'default') {
    return 'agent';
  }
  if (input.foregroundLock === true) {
    return 'agent';
  }
  if (normalizeText(input.activeSessionId) || normalizeText(input.draftAgentId) || normalizeCount(input.messageCount) > 0) {
    return 'agent';
  }
  if (normalizeText(input.worldConversationId) || normalizeCount(input.worldMessageCount) > 0) {
    return 'world';
  }
  return '';
}
