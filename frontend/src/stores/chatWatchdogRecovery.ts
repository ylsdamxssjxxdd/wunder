const normalizeEventId = (value: unknown): number => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    return 0;
  }
  return Math.max(0, Math.trunc(parsed));
};

export const shouldWatchdogReconcileDrift = (options: {
  remoteLastEventId: unknown;
  localLastEventId: unknown;
  hasPendingMessage: boolean;
}): boolean => {
  if (options.hasPendingMessage) {
    return false;
  }
  return normalizeEventId(options.remoteLastEventId) > normalizeEventId(options.localLastEventId);
};
