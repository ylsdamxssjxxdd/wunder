import {
  normalizeThreadRuntimeStatus
} from '@/utils/chatSessionRuntime';

type RuntimeLike = {
  threadStatus?: unknown;
  loaded?: boolean;
  activeTurnId?: unknown;
  watchController?: unknown;
  watchActiveRoundCount?: unknown;
  sendController?: unknown;
  resumeController?: unknown;
  compactController?: unknown;
  waitingForUserInput?: unknown;
  pendingApprovalCount?: unknown;
};

type ResolveRuntimeDerivedStatusInput = {
  runtime?: RuntimeLike | null;
  loading?: unknown;
};

export const hasRuntimeControllers = (runtime: RuntimeLike | null | undefined): boolean =>
  Boolean(runtime?.sendController || runtime?.resumeController || runtime?.compactController);

export const shouldPreserveWatchRunningStatus = (
  runtime: RuntimeLike | null | undefined,
  loading: unknown
): boolean => {
  if (!runtime || loading) return false;
  const hasWatchWork =
    Number(runtime.watchActiveRoundCount) > 0 || Boolean(String(runtime.activeTurnId || '').trim());
  return normalizeThreadRuntimeStatus(runtime.threadStatus) === 'running' &&
    Boolean(runtime.watchController) &&
    hasWatchWork;
};

export const resolveRuntimeDerivedStatus = (
  input: ResolveRuntimeDerivedStatusInput
) => {
  const runtime = input.runtime;
  if (!runtime) return 'not_loaded';
  if (runtime.waitingForUserInput) {
    return 'waiting_user_input';
  }
  if (Number(runtime.pendingApprovalCount) > 0) {
    return 'waiting_approval';
  }
  const loading = Boolean(input.loading) || hasRuntimeControllers(runtime);
  const current = normalizeThreadRuntimeStatus(runtime.threadStatus);
  if (loading) {
    return 'running';
  }
  if (current === 'system_error') {
    return current;
  }
  if (shouldPreserveWatchRunningStatus(runtime, loading)) {
    return current;
  }
  return runtime.loaded ? 'idle' : 'not_loaded';
};
