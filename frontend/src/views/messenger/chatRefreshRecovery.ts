import { chatDebugLog } from '../../utils/chatDebug';

export type SessionBusyRecoveryStatus =
  | 'already_idle'
  | 'runtime_busy'
  | 'settled'
  | 'unsettled';

type SettleAgentSessionBusyOptions = {
  sessionId: string;
  isSessionBusy: (sessionId: string) => boolean;
  resolveRuntimeStatus: (sessionId: string) => string;
  loadSessionDetail: (
    sessionId: string,
    options?: { preserveWatcher?: boolean; forceHydrateForeground?: boolean }
  ) => Promise<unknown>;
  attempts?: number;
  settleDelayMs?: number;
};

const RUNTIME_BUSY_STATUSES = new Set(['running', 'waiting_approval', 'waiting_user_input']);

const isRuntimeBusyStatus = (status: unknown): boolean =>
  RUNTIME_BUSY_STATUSES.has(
    String(status || '')
      .trim()
      .toLowerCase()
  );

const waitFor = (ms: number) =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, Math.max(0, ms));
  });

export const settleAgentSessionBusyAfterRefresh = async (
  options: SettleAgentSessionBusyOptions
): Promise<SessionBusyRecoveryStatus> => {
  const sessionId = String(options.sessionId || '').trim();
  const logScope = 'messenger.refresh-recovery';
  if (!sessionId) return 'already_idle';
  if (!options.isSessionBusy(sessionId)) {
    chatDebugLog(logScope, 'skip-already-idle', { sessionId });
    return 'already_idle';
  }

  const maxAttempts = Math.max(1, Number(options.attempts) || 2);
  const settleDelayMs = Math.max(0, Number(options.settleDelayMs) || 120);
  chatDebugLog(logScope, 'start', {
    sessionId,
    maxAttempts,
    settleDelayMs,
    initialBusy: options.isSessionBusy(sessionId),
    initialRuntimeStatus: String(options.resolveRuntimeStatus(sessionId) || '')
  });
  for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
    chatDebugLog(logScope, 'attempt', {
      sessionId,
      attempt: attempt + 1,
      maxAttempts,
      busyBefore: options.isSessionBusy(sessionId),
      runtimeBefore: String(options.resolveRuntimeStatus(sessionId) || '')
    });
    await options.loadSessionDetail(sessionId, {
      preserveWatcher: false,
      forceHydrateForeground: true
    });
    if (!options.isSessionBusy(sessionId)) {
      chatDebugLog(logScope, 'settled', {
        sessionId,
        attempt: attempt + 1,
        runtimeAfter: String(options.resolveRuntimeStatus(sessionId) || '')
      });
      return 'settled';
    }
    if (attempt + 1 < maxAttempts) {
      await waitFor(settleDelayMs * (attempt + 1));
    }
  }
  if (!options.isSessionBusy(sessionId)) {
    chatDebugLog(logScope, 'settled-after-loop', {
      sessionId,
      runtimeStatus: String(options.resolveRuntimeStatus(sessionId) || '')
    });
    return 'settled';
  }
  // If runtime still reports busy after forced hydration attempts, treat it as genuinely busy.
  const finalRuntimeStatus = String(options.resolveRuntimeStatus(sessionId) || '');
  const finalState = isRuntimeBusyStatus(finalRuntimeStatus) ? 'runtime_busy' : 'unsettled';
  chatDebugLog(logScope, 'finish', {
    sessionId,
    finalState,
    finalBusy: options.isSessionBusy(sessionId),
    finalRuntimeStatus
  });
  return finalState;
};
