export type BeeroomDispatchTargetRole = 'mother' | 'worker';

export const resolvePreferredBeeroomDispatchSessionId = (options: {
  targetRole: BeeroomDispatchTargetRole;
  targetAgentId: string;
  previousSessionId: string;
  previousTargetAgentId: string;
  activeSessionId: string;
  primarySessionId: string;
  hasExplicitPrimarySession?: boolean;
}) => {
  const targetAgentId = String(options.targetAgentId || '').trim();
  const previousSessionId = String(options.previousSessionId || '').trim();
  const previousTargetAgentId = String(options.previousTargetAgentId || '').trim();
  const activeSessionId = String(options.activeSessionId || '').trim();
  const primarySessionId = String(options.primarySessionId || '').trim();
  const hasExplicitPrimarySession = options.hasExplicitPrimarySession === true;
  if (!targetAgentId) return '';
  if (options.targetRole === 'mother') {
    if (hasExplicitPrimarySession && primarySessionId) {
      return primarySessionId;
    }
    if (previousSessionId && previousTargetAgentId === targetAgentId) {
      return previousSessionId;
    }
    return activeSessionId || primarySessionId;
  }
  if (previousSessionId && previousTargetAgentId === targetAgentId) {
    return previousSessionId;
  }
  return activeSessionId || primarySessionId;
};

export const resolveNextBeeroomMotherDispatchSessionId = (options: {
  motherAgentId: string;
  currentSessionId: string;
  currentSessionAgentId: string;
  explicitPrimarySessionId: string;
  fallbackPrimarySessionId: string;
}) => {
  const motherAgentId = String(options.motherAgentId || '').trim();
  const currentSessionId = String(options.currentSessionId || '').trim();
  const currentSessionAgentId = String(options.currentSessionAgentId || '').trim();
  const explicitPrimarySessionId = String(options.explicitPrimarySessionId || '').trim();
  const fallbackPrimarySessionId = String(options.fallbackPrimarySessionId || '').trim();
  if (!motherAgentId) return '';
  if (explicitPrimarySessionId) {
    return explicitPrimarySessionId;
  }
  if (currentSessionId && currentSessionAgentId === motherAgentId) {
    return currentSessionId;
  }
  return fallbackPrimarySessionId;
};

export const shouldFinishBeeroomTerminalHydration = (options: {
  expectedReplyText?: string;
  expectedReplyMatched?: boolean;
  baselineAssistantSignature?: string;
  assistantSignature?: string;
}) => {
  const expectedReplyText = String(options.expectedReplyText || '').trim();
  if (expectedReplyText) {
    return options.expectedReplyMatched === true;
  }
  const baselineAssistantSignature = String(options.baselineAssistantSignature || '').trim();
  const assistantSignature = String(options.assistantSignature || '').trim();
  return Boolean(assistantSignature) && assistantSignature !== baselineAssistantSignature;
};

export const shouldPreserveBeeroomDispatchPreviewOnSyncError = (options: {
  status: number;
  currentPreviewSessionId: string;
  requestedSessionId: string;
}) => {
  const status = Number(options.status || 0);
  const currentPreviewSessionId = String(options.currentPreviewSessionId || '').trim();
  const requestedSessionId = String(options.requestedSessionId || '').trim();
  return (
    currentPreviewSessionId !== '' &&
    requestedSessionId !== '' &&
    currentPreviewSessionId === requestedSessionId &&
    status !== 404 &&
    status !== 410
  );
};
