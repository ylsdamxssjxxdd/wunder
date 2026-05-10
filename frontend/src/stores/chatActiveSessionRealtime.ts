export type ActiveSessionRealtimeRecoveryPlan =
  | 'skip_no_session'
  | 'skip_inactive_session'
  | 'skip_interactive_stream'
  | 'skip_watching'
  | 'hydrate_then_watch'
  | 'watch';

type ActiveSessionRealtimeRecoveryInput = {
  targetSessionId?: unknown;
  activeSessionId?: unknown;
  hasWatchController?: unknown;
  hasSendController?: unknown;
  hasResumeController?: unknown;
  loading?: unknown;
  runtimeBusy?: unknown;
  hasPendingAssistant?: unknown;
  hasRunningAssistant?: unknown;
  hydrateIfCold?: unknown;
  forceHydrate?: unknown;
};

const normalizeSessionId = (value: unknown): string => String(value || '').trim();

const normalizeFlag = (value: unknown): boolean => {
  if (typeof value === 'string') {
    const normalized = value.trim().toLowerCase();
    if (!normalized) return false;
    return normalized !== 'false' && normalized !== '0' && normalized !== 'no';
  }
  return Boolean(value);
};

export const resolveActiveSessionRealtimeRecoveryPlan = (
  input: ActiveSessionRealtimeRecoveryInput
): ActiveSessionRealtimeRecoveryPlan => {
  const targetSessionId = normalizeSessionId(input.targetSessionId);
  if (!targetSessionId) return 'skip_no_session';

  const activeSessionId = normalizeSessionId(input.activeSessionId);
  if (!activeSessionId || activeSessionId !== targetSessionId) {
    return 'skip_inactive_session';
  }

  if (normalizeFlag(input.hasSendController) || normalizeFlag(input.hasResumeController)) {
    return 'skip_interactive_stream';
  }

  if (normalizeFlag(input.hasWatchController)) {
    return 'skip_watching';
  }

  if (normalizeFlag(input.forceHydrate)) {
    return 'hydrate_then_watch';
  }

  const localRuntimeHot =
    normalizeFlag(input.loading) ||
    normalizeFlag(input.runtimeBusy) ||
    normalizeFlag(input.hasRunningAssistant);

  // A hot session without a visible pending assistant needs a detail snapshot before watch resumes,
  // otherwise the UI can jump straight from an old bubble to a terminal server state.
  if (
    normalizeFlag(input.hydrateIfCold) &&
    localRuntimeHot &&
    !normalizeFlag(input.hasPendingAssistant)
  ) {
    return 'hydrate_then_watch';
  }

  return 'watch';
};
