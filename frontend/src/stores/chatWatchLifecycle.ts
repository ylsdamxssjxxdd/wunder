const normalizeSessionId = (value: unknown): string => String(value || '').trim();

export type ChatStreamLifecyclePhase = 'idle' | 'watching' | 'sending' | 'resuming';

export const normalizeStreamLifecyclePhase = (value: unknown): ChatStreamLifecyclePhase => {
  const normalized = String(value || '').trim().toLowerCase();
  if (
    normalized === 'idle' ||
    normalized === 'watching' ||
    normalized === 'sending' ||
    normalized === 'resuming'
  ) {
    return normalized;
  }
  return 'idle';
};

export const isRealtimeDrivenLifecycle = (value: unknown): boolean =>
  normalizeStreamLifecyclePhase(value) !== 'idle';

type RestartWatchOptions = {
  activeSessionId: unknown;
  targetSessionId: unknown;
  pageUnloading?: boolean;
};

export const shouldRestartWatchAfterInteractiveStream = (
  options: RestartWatchOptions
): boolean => {
  if (options.pageUnloading === true) {
    return false;
  }
  const active = normalizeSessionId(options.activeSessionId);
  const target = normalizeSessionId(options.targetSessionId);
  return Boolean(active && target && active === target);
};

type ForegroundHydrationOptions = {
  preserveWatcher?: boolean;
  lifecycle?: unknown;
  hasWatchController?: boolean;
  hasSendController?: boolean;
  hasResumeController?: boolean;
};

export const shouldApplyForegroundDetailHydration = (
  options: ForegroundHydrationOptions
): boolean => {
  if (options.preserveWatcher !== true) {
    return true;
  }
  if (options.hasSendController || options.hasResumeController || options.hasWatchController) {
    return false;
  }
  return !isRealtimeDrivenLifecycle(options.lifecycle);
};
