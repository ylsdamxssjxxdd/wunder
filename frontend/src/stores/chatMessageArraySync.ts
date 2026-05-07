type ChatMessage = Record<string, any>;

const normalizeSessionId = (value: unknown): string => String(value || '').trim();

// Keep realtime watchers and detail reconcile on the same array reference.
export const replaceMessageArrayKeepingReference = (
  target: ChatMessage[] | null | undefined,
  next: ChatMessage[] | null | undefined
): ChatMessage[] => {
  if (!Array.isArray(next)) {
    if (Array.isArray(target)) {
      target.splice(0, target.length);
      return target;
    }
    return [];
  }
  if (!Array.isArray(target)) {
    return next;
  }
  if (target === next) {
    return target;
  }
  target.splice(0, target.length, ...next);
  return target;
};

// Prefer the foreground array for the active session so realtime writers never drift
// onto a detached cached array after detail/history refresh swaps references.
export const resolveRealtimeMessageArrayReference = (options: {
  sessionId?: unknown;
  activeSessionId?: unknown;
  activeMessages?: ChatMessage[] | null | undefined;
  cachedMessages?: ChatMessage[] | null | undefined;
  fallbackMessages?: ChatMessage[] | null | undefined;
}): ChatMessage[] => {
  const sessionId = normalizeSessionId(options.sessionId);
  const activeSessionId = normalizeSessionId(options.activeSessionId);
  const activeMessages = Array.isArray(options.activeMessages) ? options.activeMessages : null;
  const cachedMessages = Array.isArray(options.cachedMessages) ? options.cachedMessages : null;
  const fallbackMessages = Array.isArray(options.fallbackMessages)
    ? options.fallbackMessages
    : null;

  if (sessionId && activeSessionId && sessionId === activeSessionId && activeMessages) {
    return activeMessages;
  }
  if (cachedMessages) {
    return cachedMessages;
  }
  if (fallbackMessages) {
    return fallbackMessages;
  }
  return [];
};
