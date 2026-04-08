export type BeeroomDispatchTargetRole = 'mother' | 'worker';

export const resolvePreferredBeeroomDispatchSessionId = (options: {
  targetRole: BeeroomDispatchTargetRole;
  targetAgentId: string;
  previousSessionId: string;
  previousTargetAgentId: string;
  activeSessionId: string;
  primarySessionId: string;
}) => {
  const targetAgentId = String(options.targetAgentId || '').trim();
  const previousSessionId = String(options.previousSessionId || '').trim();
  const previousTargetAgentId = String(options.previousTargetAgentId || '').trim();
  const activeSessionId = String(options.activeSessionId || '').trim();
  const primarySessionId = String(options.primarySessionId || '').trim();
  if (!targetAgentId) return '';
  if (options.targetRole === 'mother') {
    return (
      primarySessionId ||
      activeSessionId ||
      (previousTargetAgentId === targetAgentId ? previousSessionId : '')
    );
  }
  if (previousSessionId && previousTargetAgentId === targetAgentId) {
    return previousSessionId;
  }
  return activeSessionId || primarySessionId;
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
