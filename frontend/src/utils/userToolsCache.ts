import {
  fetchUserSkills,
  fetchUserToolsCatalog,
  fetchUserToolsSummary
} from '@/api/userTools';

type CacheState<T> = {
  loadedAt: number;
  promise: Promise<T | null> | null;
  value: T | null;
  version: number;
};

const USER_TOOLS_CACHE_TTL_MS = 30_000;

const createCacheState = <T>(): CacheState<T> => ({
  loadedAt: 0,
  promise: null,
  value: null,
  version: 0
});

const toolsCatalogCache = createCacheState<Record<string, unknown>>();
const toolsSummaryCache = createCacheState<Record<string, unknown>>();
const skillsCache = createCacheState<Array<Record<string, unknown>>>();

const hasFreshValue = (loadedAt: number): boolean =>
  loadedAt > 0 && Date.now() - loadedAt <= USER_TOOLS_CACHE_TTL_MS;

const resetCache = <T>(state: CacheState<T>): void => {
  state.version += 1;
  state.loadedAt = 0;
  state.promise = null;
  state.value = null;
};

const loadCache = async <T>(
  state: CacheState<T>,
  loader: () => Promise<T | null>,
  options: { force?: boolean } = {}
): Promise<T | null> => {
  const force = options.force === true;
  if (!force && state.value && hasFreshValue(state.loadedAt)) {
    return state.value;
  }
  if (state.promise) {
    return state.promise;
  }
  const version = state.version;
  state.promise = (async () => {
    const payload = await loader();
    if (version !== state.version) {
      return state.value;
    }
    state.value = payload;
    state.loadedAt = Date.now();
    return state.value;
  })().finally(() => {
    if (version === state.version) {
      state.promise = null;
    }
  });
  return state.promise;
};

export const loadUserToolsCatalogCache = (options: { force?: boolean } = {}) =>
  loadCache(
    toolsCatalogCache,
    async () => {
      const result = await fetchUserToolsCatalog();
      return ((result?.data?.data as Record<string, unknown> | null) || {}) as Record<string, unknown>;
    },
    options
  );

export const loadUserToolsSummaryCache = (options: { force?: boolean } = {}) =>
  loadCache(
    toolsSummaryCache,
    async () => {
      const result = await fetchUserToolsSummary();
      return ((result?.data?.data as Record<string, unknown> | null) || {}) as Record<string, unknown>;
    },
    options
  );

export const loadUserSkillsCache = (options: { force?: boolean } = {}) =>
  loadCache(
    skillsCache,
    async () => {
      const result = await fetchUserSkills();
      const payload = (result?.data?.data || {}) as Record<string, unknown>;
      return (Array.isArray(payload.skills) ? payload.skills : []).filter(
        (item): item is Record<string, unknown> => Boolean(item && typeof item === 'object')
      );
    },
    options
  );

export const invalidateUserToolsCatalogCache = (): void => {
  resetCache(toolsCatalogCache);
};

export const invalidateUserToolsSummaryCache = (): void => {
  resetCache(toolsSummaryCache);
};

export const invalidateUserSkillsCache = (): void => {
  resetCache(skillsCache);
};

export const invalidateAllUserToolsCaches = (): void => {
  invalidateUserToolsCatalogCache();
  invalidateUserToolsSummaryCache();
  invalidateUserSkillsCache();
};
