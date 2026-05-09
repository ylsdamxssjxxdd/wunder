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
