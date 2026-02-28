const WORKSPACE_IMAGE_CACHE_NAME = 'wunder.workspace.image.v1';
const WORKSPACE_IMAGE_CACHE_PREFIX = '/__wunder_workspace_image_cache__/';
const WORKSPACE_IMAGE_CACHE_FILENAME_HEADER = 'x-wunder-filename';
const WORKSPACE_IMAGE_CACHE_CREATED_AT_HEADER = 'x-wunder-created-at';
const WORKSPACE_IMAGE_CACHE_TTL_MS = 24 * 60 * 60 * 1000;
const WORKSPACE_IMAGE_CACHE_MAX_ITEMS = 120;

export type WorkspaceImagePersistentCachePayload = {
  blob: Blob;
  filename: string;
};

type WorkspaceImageCacheKeyInput = {
  scope?: unknown;
  publicPath?: unknown;
  requestUserId?: unknown;
  requestAgentId?: unknown;
};

const canUsePersistentCache = () =>
  typeof window !== 'undefined' && typeof window.caches !== 'undefined';

const getCacheOrigin = () => {
  if (typeof window !== 'undefined' && window.location?.origin) {
    return window.location.origin;
  }
  return 'https://wunder.local';
};

const toCacheRequest = (cacheKey: string) => {
  const normalized = encodeURIComponent(String(cacheKey || ''));
  return new Request(`${getCacheOrigin()}${WORKSPACE_IMAGE_CACHE_PREFIX}${normalized}`);
};

const parseCreatedAt = (raw: string | null) => {
  const value = Number(raw || 0);
  if (!Number.isFinite(value) || value <= 0) return 0;
  return value;
};

const decodeHeaderValue = (raw: string | null) => {
  const value = String(raw || '').trim();
  if (!value) return '';
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
};

const encodeHeaderValue = (raw: string) => {
  const value = String(raw || '').trim();
  if (!value) return '';
  try {
    return encodeURIComponent(value);
  } catch {
    return value;
  }
};

const prunePersistentCache = async (cache: Cache) => {
  const keys = await cache.keys();
  if (keys.length <= WORKSPACE_IMAGE_CACHE_MAX_ITEMS) return;
  const records = await Promise.all(
    keys.map(async (request) => {
      const response = await cache.match(request);
      return {
        request,
        createdAt: parseCreatedAt(response?.headers.get(WORKSPACE_IMAGE_CACHE_CREATED_AT_HEADER) || null)
      };
    })
  );
  records.sort((left, right) => left.createdAt - right.createdAt);
  const overflow = records.length - WORKSPACE_IMAGE_CACHE_MAX_ITEMS;
  for (let index = 0; index < overflow; index += 1) {
    const request = records[index]?.request;
    if (!request) continue;
    await cache.delete(request);
  }
};

export const buildWorkspaceImagePersistentCacheKey = (
  input: WorkspaceImageCacheKeyInput = {}
) => {
  const scope = String(input.scope || '').trim();
  const userId = String(input.requestUserId || '').trim();
  const agentId = String(input.requestAgentId || '').trim();
  const publicPath = String(input.publicPath || '').trim();
  return `${scope}|${userId}|${agentId}|${publicPath}`;
};

export const readWorkspaceImagePersistentCache = async (
  cacheKey: string
): Promise<WorkspaceImagePersistentCachePayload | null> => {
  if (!cacheKey || !canUsePersistentCache()) return null;
  try {
    const cache = await window.caches.open(WORKSPACE_IMAGE_CACHE_NAME);
    const request = toCacheRequest(cacheKey);
    const response = await cache.match(request);
    if (!response) return null;
    const createdAt = parseCreatedAt(
      response.headers.get(WORKSPACE_IMAGE_CACHE_CREATED_AT_HEADER)
    );
    if (createdAt > 0 && Date.now() - createdAt > WORKSPACE_IMAGE_CACHE_TTL_MS) {
      await cache.delete(request);
      return null;
    }
    const blob = await response.blob();
    if (!(blob instanceof Blob) || blob.size <= 0) {
      await cache.delete(request);
      return null;
    }
    return {
      blob,
      filename: decodeHeaderValue(response.headers.get(WORKSPACE_IMAGE_CACHE_FILENAME_HEADER))
    };
  } catch {
    return null;
  }
};

export const writeWorkspaceImagePersistentCache = async (
  cacheKey: string,
  payload: WorkspaceImagePersistentCachePayload
) => {
  if (!cacheKey || !canUsePersistentCache()) return;
  const blob = payload?.blob;
  if (!(blob instanceof Blob) || blob.size <= 0) return;
  try {
    const cache = await window.caches.open(WORKSPACE_IMAGE_CACHE_NAME);
    const request = toCacheRequest(cacheKey);
    const headers = new Headers({
      'content-type': blob.type || 'application/octet-stream',
      [WORKSPACE_IMAGE_CACHE_FILENAME_HEADER]: encodeHeaderValue(payload.filename),
      [WORKSPACE_IMAGE_CACHE_CREATED_AT_HEADER]: String(Date.now())
    });
    await cache.put(request, new Response(blob, { headers }));
    await prunePersistentCache(cache);
  } catch {
    // Ignore private-mode and quota failures.
  }
};
