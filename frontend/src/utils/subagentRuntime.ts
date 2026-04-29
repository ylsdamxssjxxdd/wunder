type SubagentLike = Record<string, unknown>;

const ACTIVE_STATUSES = new Set([
  'accepted',
  'in_progress',
  'inprogress',
  'loading',
  'pending',
  'processing',
  'queued',
  'running',
  'started',
  'waiting'
]);

const FAILED_STATUSES = new Set([
  'aborted',
  'cancelled',
  'canceled',
  'closed',
  'error',
  'failed',
  'not_found',
]);

const SUCCESS_STATUSES = new Set([
  'complete',
  'completed',
  'done',
  'finished',
  'idle',
  'success',
  'succeeded'
]);

export const normalizeSubagentRuntimeFlag = (value: unknown): boolean => {
  if (typeof value === 'string') {
    const text = value.trim().toLowerCase();
    if (!text) return false;
    return text !== 'false' && text !== '0' && text !== 'no';
  }
  return Boolean(value);
};

export const normalizeSubagentRuntimeStatus = (value: unknown): string =>
  String(value || '').trim().toLowerCase();

export const isSubagentStatusActive = (value: unknown): boolean =>
  ACTIVE_STATUSES.has(normalizeSubagentRuntimeStatus(value));

export const isSubagentStatusFailed = (value: unknown): boolean =>
  FAILED_STATUSES.has(normalizeSubagentRuntimeStatus(value));

export const isSubagentStatusSuccessful = (value: unknown): boolean =>
  SUCCESS_STATUSES.has(normalizeSubagentRuntimeStatus(value));

export const isSubagentItemActive = (item: SubagentLike | null | undefined): boolean => {
  if (!item || typeof item !== 'object') return false;
  const status = normalizeSubagentRuntimeStatus(
    item.status ?? (item.agent_state as SubagentLike | undefined)?.status
  );
  if (isSubagentStatusActive(status)) return true;
  if (status === 'timeout') {
    return !normalizeSubagentRuntimeFlag(item.terminal) && !normalizeSubagentRuntimeFlag(item.failed);
  }
  if (isSubagentStatusFailed(status) || isSubagentStatusSuccessful(status)) return false;
  if (normalizeSubagentRuntimeFlag(item.terminal) || normalizeSubagentRuntimeFlag(item.failed)) return false;
  if (normalizeSubagentRuntimeFlag(item.reply_pending) || normalizeSubagentRuntimeFlag(item.replyPending)) {
    return true;
  }
  return Boolean(String(item.session_id ?? item.sessionId ?? item.run_id ?? item.runId ?? '').trim());
};

export const hasActiveSubagentItems = (items: unknown): boolean =>
  Array.isArray(items) && items.some((item) => isSubagentItemActive(item as SubagentLike));
