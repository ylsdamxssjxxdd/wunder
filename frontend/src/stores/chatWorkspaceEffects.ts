import { emitWorkspaceRefresh } from '@/utils/workspaceEvents';
import { normalizeThreadRuntimeStatus } from '@/utils/chatSessionRuntime';

const refreshStatuses = new Set([
  'completed', 'failed', 'cancelled', 'canceled', 'interrupted', 'rejected', 'stopped',
  'idle', 'not_loaded', 'system_error', 'waiting_input', 'waiting_user_input', 'waiting_approval'
]);
const terminalStatuses = new Set(['completed', 'failed', 'cancelled', 'rejected', 'stopped']);
// Final artifacts can land just after the terminal frame; one trailing
// directory refresh per session guarantees the panel ends on the committed
// state without rescanning on every token update.
const COMPLETION_SETTLE_REFRESH_MS = 1500;
const completionSettleTimers = new Map<string, ReturnType<typeof setTimeout>>();

/** Only execution boundaries invalidate directories; token updates never do. */
export const shouldRefreshWorkspaceForThreadStatus = (previous: unknown, next: unknown): boolean => {
  const before = String(previous ?? '').toLowerCase();
  const after = String(next ?? '').toLowerCase();
  return normalizeThreadRuntimeStatus(before) !== normalizeThreadRuntimeStatus(after) && refreshStatuses.has(after) &&
    (!['idle', 'not_loaded'].includes(after) ||
      ['running', 'queued', 'waiting_input', 'waiting_user_input', 'waiting_approval'].includes(before));
};

export const refreshWorkspaceForThreadStatus = (store, sessionId: string, status: unknown): void => {
  const agentId = store.sessions?.find(session => session.id === sessionId)?.agent_id;
  const detail = {
    sessionId,
    ...(agentId ? { agentId } : {}),
    reason: `thread_${String(status)}`,
    forceTree: true,
    // A lifecycle boundary invalidates the directory snapshot, without
    // forcing every visible chat image to download again.
    scope: 'directory'
  };
  emitWorkspaceRefresh(detail);
  const normalized = normalizeThreadRuntimeStatus(status);
  if (!terminalStatuses.has(normalized)) return;
  const pending = completionSettleTimers.get(sessionId);
  if (pending) clearTimeout(pending);
  completionSettleTimers.set(sessionId, setTimeout(() => {
    completionSettleTimers.delete(sessionId);
    emitWorkspaceRefresh({ ...detail, reason: `thread_${normalized}_settle` });
  }, COMPLETION_SETTLE_REFRESH_MS));
};
