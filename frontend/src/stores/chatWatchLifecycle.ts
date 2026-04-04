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

type ActiveSessionPreserveOptions = {
  isSameActiveSession?: boolean;
  lifecycle?: unknown;
  hasSendController?: boolean;
  hasResumeController?: boolean;
};

export const shouldForcePreserveWatcherForActiveSession = (
  options: ActiveSessionPreserveOptions
): boolean => {
  if (options.isSameActiveSession !== true) {
    return false;
  }
  if (options.hasSendController || options.hasResumeController) {
    return true;
  }
  const lifecycle = normalizeStreamLifecyclePhase(options.lifecycle);
  return lifecycle === 'sending' || lifecycle === 'resuming';
};

export const shouldApplyForegroundDetailHydration = (
  options: ForegroundHydrationOptions
): boolean => {
  if (options.preserveWatcher !== true) {
    return true;
  }
  if (options.hasSendController || options.hasResumeController) {
    return false;
  }
  const lifecycle = normalizeStreamLifecyclePhase(options.lifecycle);
  return lifecycle !== 'sending' && lifecycle !== 'resuming';
};
