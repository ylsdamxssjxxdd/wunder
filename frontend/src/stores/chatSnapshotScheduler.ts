export type ChatSnapshotScheduleContext = {
  sessionId: string;
  fallbackMessages: unknown[];
};

const normalizeSessionId = (value: unknown): string => String(value ?? '').trim();

const normalizeMessages = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);

export const captureChatSnapshotScheduleContext = (
  storeState: { activeSessionId?: unknown; messages?: unknown[] } | null | undefined
): ChatSnapshotScheduleContext | null => {
  const sessionId = normalizeSessionId(storeState?.activeSessionId);
  if (!sessionId) {
    return null;
  }
  return {
    sessionId,
    fallbackMessages: normalizeMessages(storeState?.messages)
  };
};

export const resolveChatSnapshotScheduleSource = (
  storeState: { activeSessionId?: unknown; messages?: unknown[] } | null | undefined,
  context: ChatSnapshotScheduleContext | null | undefined,
  readSessionMessages: (sessionId: string) => unknown[] | null | undefined
): { sessionId: string; messages: unknown[] } | null => {
  const sessionId = normalizeSessionId(context?.sessionId ?? storeState?.activeSessionId);
  if (!sessionId) {
    return null;
  }
  const activeSessionId = normalizeSessionId(storeState?.activeSessionId);
  if (activeSessionId && activeSessionId === sessionId) {
    return {
      sessionId,
      messages: normalizeMessages(storeState?.messages)
    };
  }
  const cachedMessages = normalizeMessages(readSessionMessages(sessionId) ?? context?.fallbackMessages);
  return {
    sessionId,
    messages: cachedMessages
  };
};
