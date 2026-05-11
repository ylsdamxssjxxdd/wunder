export const ALL_SESSION_LIST_CACHE_KEY = '__all_sessions__';
export const LOAD_SESSIONS_BACKGROUND_REFRESH_MIN_AGE_MS = 5000;
export const DEFAULT_AGENT_CACHE_KEY = '__default__';

export const resolveLoadSessionsCacheKey = (
  agentId: string | null,
  resolveAgentCacheKey: (agentId: string) => string
): string => {
  if (agentId === null) {
    return ALL_SESSION_LIST_CACHE_KEY;
  }
  return resolveAgentCacheKey(agentId);
};

export const normalizeSessionListItems = (items: unknown): Record<string, unknown>[] =>
  (Array.isArray(items) ? items : []).filter(
    (item): item is Record<string, unknown> =>
      Boolean(item) && typeof item === 'object' && !Array.isArray(item)
  );

export const isAllSessionsCacheKeyCollidingWithDefaultAgent = (
  resolveAgentCacheKey: (agentId: string) => string
): boolean => ALL_SESSION_LIST_CACHE_KEY === resolveAgentCacheKey(DEFAULT_AGENT_CACHE_KEY);
