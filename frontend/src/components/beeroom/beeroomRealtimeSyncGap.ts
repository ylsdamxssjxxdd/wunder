const normalizeMs = (value: unknown): number => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return 0;
  return Math.max(0, Math.floor(parsed));
};

export const shouldRunSyncRequiredReloadImmediately = (
  nowMs: unknown,
  lastRunMs: unknown,
  throttleMs: unknown
): boolean => {
  const now = normalizeMs(nowMs);
  const last = normalizeMs(lastRunMs);
  const throttle = normalizeMs(throttleMs);
  const elapsed = Math.max(0, now - last);
  return elapsed >= throttle;
};

export const resolveSyncRequiredReloadDelayMs = (
  nowMs: unknown,
  lastRunMs: unknown,
  throttleMs: unknown
): number => {
  if (shouldRunSyncRequiredReloadImmediately(nowMs, lastRunMs, throttleMs)) {
    return 0;
  }
  const now = normalizeMs(nowMs);
  const last = normalizeMs(lastRunMs);
  const throttle = normalizeMs(throttleMs);
  const elapsed = Math.max(0, now - last);
  const remaining = Math.max(0, throttle - elapsed);
  return Math.max(80, Math.floor(remaining));
};
