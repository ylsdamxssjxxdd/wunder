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
  const remote = normalizeEventId(options.remoteLastEventId);
  const local = normalizeEventId(options.localLastEventId);
  // Ignore tiny drift after terminal hydrate paths. A 1-step delta is commonly caused by
  // server-side terminal bookkeeping landing after the foreground message merge, and forcing
  // detail reload on every watchdog tick can repeatedly resurrect stale assistant content.
  return remote > local + 1;
};
