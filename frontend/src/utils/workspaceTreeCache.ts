import { fetchWunderWorkspaceContent } from '@/api/workspace';

export const WORKSPACE_TREE_CACHE_TTL_MS = 5 * 60 * 1000;
export const DEFAULT_WORKSPACE_AGENT_KEY = '__default__';

const WORKSPACE_TREE_CACHE_STORAGE_KEY = 'wunder.workspace.tree_cache';
const WORKSPACE_TREE_CACHE_MAX_ITEMS = 24;

export type WorkspaceTreeCacheEntry = {
  cachedAt: number;
  path: string;
  parent: string | null;
  entries: unknown[];
};

type WorkspaceTreeCacheShape = Record<string, WorkspaceTreeCacheEntry>;

type PrefetchWorkspaceTreeOptions = {
  agentId?: string;
  path?: string;
  sortBy?: string;
  sortOrder?: string;
};

const workspaceTreeCache = new Map<string, WorkspaceTreeCacheEntry>();
const workspaceTreePrefetchInFlight = new Map<string, Promise<WorkspaceTreeCacheEntry | null>>();
let workspaceTreeCacheHydrated = false;

export const cloneWorkspaceEntries = (entries: unknown[]) => {
  if (!Array.isArray(entries)) return [];
  if (typeof structuredClone === 'function') {
    try {
      return structuredClone(entries);
    } catch (error) {
      // Fallback to JSON clone when structuredClone fails.
    }
  }
  try {
    return JSON.parse(JSON.stringify(entries));
  } catch (error) {
    return entries.slice();
  }
};

export const normalizeWorkspacePath = (path: unknown) => {
  if (!path) return '';
  return String(path).replace(/\\/g, '/').replace(/^\/+/, '');
};

const normalizeWorkspaceAgentKey = (agentId: unknown) => {
  const normalized = String(agentId || '').trim();
  return normalized || DEFAULT_WORKSPACE_AGENT_KEY;
};

const normalizeSortBy = (value: unknown) => String(value || 'name').trim() || 'name';
const normalizeSortOrder = (value: unknown) =>
  String(value || 'asc').trim().toLowerCase() === 'desc' ? 'desc' : 'asc';

const pruneWorkspaceTreeCache = (now = Date.now()) => {
  workspaceTreeCache.forEach((entry, cacheKey) => {
    if (!entry || !Number.isFinite(entry.cachedAt) || now - entry.cachedAt > WORKSPACE_TREE_CACHE_TTL_MS) {
      workspaceTreeCache.delete(cacheKey);
    }
  });

  if (workspaceTreeCache.size <= WORKSPACE_TREE_CACHE_MAX_ITEMS) return;

  const sorted = Array.from(workspaceTreeCache.entries()).sort(
    (left, right) => left[1].cachedAt - right[1].cachedAt
  );
  const overflow = workspaceTreeCache.size - WORKSPACE_TREE_CACHE_MAX_ITEMS;
  for (let i = 0; i < overflow; i += 1) {
    const cacheKey = sorted[i]?.[0];
    if (cacheKey) {
      workspaceTreeCache.delete(cacheKey);
    }
  }
};

const persistWorkspaceTreeCache = () => {
  if (typeof window === 'undefined') return;
  try {
    pruneWorkspaceTreeCache();
    const payload: WorkspaceTreeCacheShape = {};
    workspaceTreeCache.forEach((entry, cacheKey) => {
      payload[cacheKey] = entry;
    });
    window.sessionStorage.setItem(WORKSPACE_TREE_CACHE_STORAGE_KEY, JSON.stringify(payload));
  } catch (error) {
    // Ignore quota/private-mode errors and keep in-memory cache only.
  }
};

const hydrateWorkspaceTreeCache = () => {
  if (workspaceTreeCacheHydrated) return;
  workspaceTreeCacheHydrated = true;
  if (typeof window === 'undefined') return;
  try {
    const raw = window.sessionStorage.getItem(WORKSPACE_TREE_CACHE_STORAGE_KEY);
    if (!raw) return;
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') return;
    const now = Date.now();
    Object.entries(parsed as WorkspaceTreeCacheShape).forEach(([cacheKey, entry]) => {
      if (!entry || typeof entry !== 'object') return;
      const cachedAt = Number(entry.cachedAt);
      if (!Number.isFinite(cachedAt) || now - cachedAt > WORKSPACE_TREE_CACHE_TTL_MS) return;
      workspaceTreeCache.set(cacheKey, {
        cachedAt,
        path: normalizeWorkspacePath(entry.path),
        parent: entry.parent ? normalizeWorkspacePath(entry.parent) : null,
        entries: cloneWorkspaceEntries(entry.entries || [])
      });
    });
    pruneWorkspaceTreeCache(now);
  } catch (error) {
    workspaceTreeCache.clear();
  }
};

export const buildWorkspaceTreeCacheKey = (
  agentId: unknown,
  path: unknown,
  sortBy: unknown,
  sortOrder: unknown
) => {
  const safeAgentId = normalizeWorkspaceAgentKey(agentId);
  const safePath = normalizeWorkspacePath(path);
  const safeSortBy = normalizeSortBy(sortBy);
  const safeSortOrder = normalizeSortOrder(sortOrder);
  return `${safeAgentId}|${safePath}|${safeSortBy}|${safeSortOrder}`;
};

export const readWorkspaceTreeCache = (cacheKey: string): WorkspaceTreeCacheEntry | null => {
  hydrateWorkspaceTreeCache();
  const cached = workspaceTreeCache.get(cacheKey);
  if (!cached) return null;
  if (Date.now() - cached.cachedAt > WORKSPACE_TREE_CACHE_TTL_MS) {
    workspaceTreeCache.delete(cacheKey);
    persistWorkspaceTreeCache();
    return null;
  }
  return {
    cachedAt: cached.cachedAt,
    path: normalizeWorkspacePath(cached.path),
    parent: cached.parent ? normalizeWorkspacePath(cached.parent) : null,
    entries: cloneWorkspaceEntries(cached.entries || [])
  };
};

export const writeWorkspaceTreeCache = (
  cacheKey: string,
  payload: Partial<WorkspaceTreeCacheEntry> = {}
) => {
  hydrateWorkspaceTreeCache();
  workspaceTreeCache.set(cacheKey, {
    cachedAt: Date.now(),
    path: normalizeWorkspacePath(payload.path),
    parent: payload.parent ? normalizeWorkspacePath(payload.parent) : null,
    entries: cloneWorkspaceEntries(payload.entries || [])
  });
  persistWorkspaceTreeCache();
};

export const prefetchWorkspaceTree = async (
  options: PrefetchWorkspaceTreeOptions = {}
): Promise<WorkspaceTreeCacheEntry | null> => {
  const agentId = String(options.agentId || '').trim();
  const path = normalizeWorkspacePath(options.path);
  const sortBy = normalizeSortBy(options.sortBy);
  const sortOrder = normalizeSortOrder(options.sortOrder);
  const cacheKey = buildWorkspaceTreeCacheKey(agentId, path, sortBy, sortOrder);
  const cached = readWorkspaceTreeCache(cacheKey);
  if (cached) return cached;

  const inFlight = workspaceTreePrefetchInFlight.get(cacheKey);
  if (inFlight) return inFlight;

  const request = (async () => {
    try {
      const params: Record<string, string | number | boolean> = {
        path,
        include_content: true,
        depth: 1,
        sort_by: sortBy,
        order: sortOrder
      };
      if (agentId) {
        params.agent_id = agentId;
      }
      const { data } = await fetchWunderWorkspaceContent(params);
      const payload = data || {};
      const normalizedPath = normalizeWorkspacePath(payload.path ?? path);
      const normalizedParent = normalizedPath
        ? normalizeWorkspacePath(normalizedPath.split('/').slice(0, -1).join('/'))
        : '';
      const normalizedEntries = Array.isArray(payload.entries) ? payload.entries : [];
      writeWorkspaceTreeCache(cacheKey, {
        path: normalizedPath,
        parent: normalizedParent || null,
        entries: normalizedEntries
      });
      if (normalizedPath !== path) {
        const resolvedCacheKey = buildWorkspaceTreeCacheKey(agentId, normalizedPath, sortBy, sortOrder);
        writeWorkspaceTreeCache(resolvedCacheKey, {
          path: normalizedPath,
          parent: normalizedParent || null,
          entries: normalizedEntries
        });
      }
      return readWorkspaceTreeCache(cacheKey);
    } catch (error) {
      return null;
    } finally {
      workspaceTreePrefetchInFlight.delete(cacheKey);
    }
  })();

  workspaceTreePrefetchInFlight.set(cacheKey, request);
  return request;
};
