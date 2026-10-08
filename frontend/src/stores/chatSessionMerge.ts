import { mergeSessionQuotaUsed } from './chatSessionQuota';

export type ChatSessionLike = Record<string, unknown> & {
  id?: unknown;
};

export const resolveChatSessionKey = (value: unknown): string => String(value || '').trim();

export const mergeSessionRuntimeFields = (
  current: ChatSessionLike | null | undefined,
  incoming: ChatSessionLike | null | undefined
): ChatSessionLike => {
  const currentRecord =
    current && typeof current === 'object' && !Array.isArray(current)
      ? (current as ChatSessionLike)
      : {};
  const incomingRecord =
    incoming && typeof incoming === 'object' && !Array.isArray(incoming)
      ? (incoming as ChatSessionLike)
      : {};
  const merged = {
    ...currentRecord,
    ...incomingRecord
  } as ChatSessionLike;
  const runtimeKey = (record: ChatSessionLike): string => String(
    record.runtime_status ?? record.runtimeStatus ?? record.thread_status ?? record.threadStatus ?? ''
  ).trim().toLowerCase();
  const currentRuntime = runtimeKey(currentRecord);
  const incomingRuntime = runtimeKey(incomingRecord);
  const terminalOrBusy = new Set([
    'running', 'queued', 'waiting', 'waiting_approval', 'waiting_user_input',
    'completed', 'finished', 'done', 'failed', 'error', 'cancelled', 'canceled',
    'interrupted', 'stopped'
  ]);
  // A catalog refresh can race a terminal frame and briefly return only the
  // generic session `active` state. Do not erase a known runtime state with
  // that transport placeholder; a real running/terminal value still wins.
  if (terminalOrBusy.has(currentRuntime) &&
      (!incomingRuntime || ['active', 'idle', 'not_loaded'].includes(incomingRuntime))) {
    merged.runtime_status = currentRecord.runtime_status ?? currentRecord.runtimeStatus;
    merged.runtimeStatus = currentRecord.runtimeStatus ?? currentRecord.runtime_status;
    if (currentRecord.thread_status !== undefined) merged.thread_status = currentRecord.thread_status;
    if (currentRecord.threadStatus !== undefined) merged.threadStatus = currentRecord.threadStatus;
  }
  const quotaUsed = mergeSessionQuotaUsed(currentRecord, incomingRecord);
  if (quotaUsed !== null) {
    merged.model_request_count = quotaUsed;
    merged.quota_used = quotaUsed;
  }

  const contextKeys = [
    'context_tokens',
    'context_occupancy_tokens',
    'contextTokens',
    'contextOccupancyTokens',
    'context_max_tokens',
    'context_total_tokens',
    'contextTotalTokens',
    'max_context',
    'maxContext',
    'context_window'
  ] as const;
  contextKeys.forEach((key) => {
    if (
      (incomingRecord[key] === null || incomingRecord[key] === undefined || incomingRecord[key] === '') &&
      currentRecord[key] !== null &&
      currentRecord[key] !== undefined &&
      currentRecord[key] !== ''
    ) {
      merged[key] = currentRecord[key];
    }
  });

  if (
    (incomingRecord.goal === null || incomingRecord.goal === undefined) &&
    currentRecord.goal !== null &&
    currentRecord.goal !== undefined
  ) {
    merged.goal = currentRecord.goal;
  }
  if (
    (incomingRecord.orchestration_lock === null || incomingRecord.orchestration_lock === undefined) &&
    currentRecord.orchestration_lock !== null &&
    currentRecord.orchestration_lock !== undefined
  ) {
    merged.orchestration_lock = currentRecord.orchestration_lock;
  }
  return merged;
};

export const mergeSessionsByIdPreservingRuntimeFields = (
  currentSessions: ChatSessionLike[] | null | undefined,
  incomingSessions: ChatSessionLike[] | null | undefined,
  patchSession: (session: ChatSessionLike | null | undefined) => ChatSessionLike,
  sortSessions: (sessions: ChatSessionLike[]) => ChatSessionLike[]
): ChatSessionLike[] => {
  const currentList = Array.isArray(currentSessions) ? currentSessions : [];
  const incomingList = Array.isArray(incomingSessions) ? incomingSessions : [];
  if (!currentList.length) {
    return sortSessions(incomingList.map((item) => patchSession(item)));
  }
  const currentById = new Map<string, ChatSessionLike>();
  currentList.forEach((session) => {
    const key = resolveChatSessionKey(session?.id);
    if (!key) return;
    currentById.set(key, session);
  });
  const merged = incomingList.map((session) => {
    const key = resolveChatSessionKey(session?.id);
    if (!key) {
      return patchSession(session);
    }
    return patchSession(mergeSessionRuntimeFields(currentById.get(key) || null, session));
  });
  return sortSessions(merged);
};
