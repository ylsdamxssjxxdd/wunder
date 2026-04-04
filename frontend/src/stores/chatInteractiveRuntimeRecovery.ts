const normalizeTimestampMs = (value: unknown): number => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    return 0;
  }
  return Math.max(0, Math.trunc(parsed));
};

const normalizeEventId = (value: unknown): number => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    return 0;
  }
  return Math.max(0, Math.trunc(parsed));
};

const REMOTE_AHEAD_IDLE_MS = 6000;
const REMOTE_AHEAD_ACTIVE_MS = 6000;
const NO_LOADING_STALE_IDLE_MS = 12000;
const NO_LOADING_STALE_ACTIVE_MS = 15000;
const HARD_STALE_IDLE_MS = 45000;
const HARD_STALE_ACTIVE_MS = 90000;

export type InteractiveRecoveryReason =
  | ''
  | 'aborted'
  | 'remote_idle'
  | 'remote_ahead'
  | 'not_loading_stale'
  | 'hard_stale';

export const resolveInteractiveControllerRecoveryReason = (options: {
  hasController: boolean;
  controllerAborted?: boolean;
  startedAt?: unknown;
  lastEventAt?: unknown;
  loading?: boolean;
  remoteRunning?: unknown;
  remoteLastEventId?: unknown;
  localLastEventId?: unknown;
  nowMs?: number;
}): InteractiveRecoveryReason => {
  if (options.hasController !== true) {
    return '';
  }
  if (options.controllerAborted === true) {
    return 'aborted';
  }
  if (options.remoteRunning === false) {
    return 'remote_idle';
  }
  const nowMsRaw = normalizeTimestampMs(options.nowMs);
  const nowMs = nowMsRaw > 0 ? nowMsRaw : Date.now();
  const startedAt = normalizeTimestampMs(options.startedAt);
  const lastEventAtRaw = normalizeTimestampMs(options.lastEventAt);
  const lastEventAt = lastEventAtRaw > 0 ? lastEventAtRaw : startedAt;
  const activeMs = startedAt > 0 ? Math.max(0, nowMs - startedAt) : 0;
  const idleMs = lastEventAt > 0 ? Math.max(0, nowMs - lastEventAt) : activeMs;
  const remoteLastEventId = normalizeEventId(options.remoteLastEventId);
  const localLastEventId = normalizeEventId(options.localLastEventId);
  if (
    remoteLastEventId > localLastEventId &&
    activeMs >= REMOTE_AHEAD_ACTIVE_MS &&
    idleMs >= REMOTE_AHEAD_IDLE_MS
  ) {
    return 'remote_ahead';
  }
  if (
    options.loading !== true &&
    activeMs >= NO_LOADING_STALE_ACTIVE_MS &&
    idleMs >= NO_LOADING_STALE_IDLE_MS
  ) {
    return 'not_loading_stale';
  }
  if (activeMs >= HARD_STALE_ACTIVE_MS && idleMs >= HARD_STALE_IDLE_MS) {
    return 'hard_stale';
  }
  return '';
};

export const shouldRecoverInteractiveController = (options: {
  hasController: boolean;
  controllerAborted?: boolean;
  startedAt?: unknown;
  lastEventAt?: unknown;
  loading?: boolean;
  remoteRunning?: unknown;
  remoteLastEventId?: unknown;
  localLastEventId?: unknown;
  nowMs?: number;
}): boolean => Boolean(resolveInteractiveControllerRecoveryReason(options));
