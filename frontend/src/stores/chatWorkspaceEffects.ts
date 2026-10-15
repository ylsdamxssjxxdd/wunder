import { emitWorkspaceRefresh } from '@/utils/workspaceEvents';
import { normalizeThreadRuntimeStatus } from '@/utils/chatSessionRuntime';

const refreshStatuses = new Set([
  'completed', 'failed', 'cancelled', 'canceled', 'interrupted', 'rejected', 'stopped',
  'idle', 'not_loaded', 'system_error', 'waiting_input', 'waiting_user_input', 'waiting_approval'
]);

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
  emitWorkspaceRefresh({
    sessionId,
    ...(agentId ? { agentId } : {}),
    reason: `thread_${String(status)}`,
    forceTree: true,
    // A lifecycle boundary invalidates the directory snapshot, without
    // forcing every visible chat image to download again.
    scope: 'directory'
  });
};
