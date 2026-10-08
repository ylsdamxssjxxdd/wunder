// Scope cached HTML and hydrated content to the owning projection lifetime, not a row.
const caches = new WeakMap<object, ReturnType<typeof createMessageMarkdownCache>>();
export const useMessageMarkdownCache = (owner: object) => {
  let cache = caches.get(owner);
  if (!cache) {
    cache = createMessageMarkdownCache();
    caches.set(owner, cache);
  }
  return cache;
};

const createMessageMarkdownCache = () => {
  type RenderCacheEntry = {
    source: string;
    html: string;
    updatedAt: number;
    bytes: number;
  };

  type HydratedHistoryContent = {
    content: string;
    bytes: number;
  };

  const MARKDOWN_BODY_CACHE_LIMIT = 96;
  const MARKDOWN_BODY_CACHE_MAX_BYTES = 12 * 1024 * 1024;
  const HYDRATED_HISTORY_CONTENT_CACHE_LIMIT = 64;
  const HYDRATED_HISTORY_CONTENT_CACHE_MAX_BYTES = 8 * 1024 * 1024;
  const streamingMarkdownCache = new Map<string, RenderCacheEntry>();
  const hydratedHistoryContentCache = new Map<string, HydratedHistoryContent>();
  let streamingMarkdownCacheBytes = 0;
  let hydratedHistoryContentCacheBytes = 0;
  const trimStreamingMarkdownCache = () => {
    while (
      streamingMarkdownCache.size > MARKDOWN_BODY_CACHE_LIMIT ||
      streamingMarkdownCacheBytes > MARKDOWN_BODY_CACHE_MAX_BYTES
    ) {
      const oldestKey = streamingMarkdownCache.keys().next().value as string | undefined;
      if (!oldestKey) break;
      const oldest = streamingMarkdownCache.get(oldestKey);
      if (oldest) streamingMarkdownCacheBytes -= oldest.bytes;
      streamingMarkdownCache.delete(oldestKey);
    }
  };

  const deleteMarkdownCacheEntry = (key: string) => {
    const cached = streamingMarkdownCache.get(key);
    if (cached) streamingMarkdownCacheBytes -= cached.bytes;
    streamingMarkdownCache.delete(key);
  };

  const readMarkdownCacheEntry = (key: string): RenderCacheEntry | null => {
    const cached = streamingMarkdownCache.get(key);
    if (!cached) return null;
    // Refresh the LRU order without duplicating the stored HTML string.
    streamingMarkdownCache.delete(key);
    streamingMarkdownCache.set(key, cached);
    return cached;
  };

  const writeMarkdownCacheEntry = (key: string, source: string, html: string) => {
    deleteMarkdownCacheEntry(key);
    streamingMarkdownCache.set(key, {
      source,
      html,
      updatedAt: Date.now(),
      bytes: source.length * 2 + html.length * 2
    });
    streamingMarkdownCacheBytes += source.length * 2 + html.length * 2;
    trimStreamingMarkdownCache();
  };

  const readHydratedHistoryContent = (key: string): string | null => {
    const cached = hydratedHistoryContentCache.get(key);
    if (!cached) return null;
    hydratedHistoryContentCache.delete(key);
    hydratedHistoryContentCache.set(key, cached);
    return cached.content;
  };

  const writeHydratedHistoryContent = (key: string, content: string) => {
    const previous = hydratedHistoryContentCache.get(key);
    if (previous) hydratedHistoryContentCacheBytes -= previous.bytes;
    const entry = { content, bytes: content.length * 2 };
    hydratedHistoryContentCache.set(key, entry);
    hydratedHistoryContentCacheBytes += entry.bytes;
    while (
      hydratedHistoryContentCache.size > HYDRATED_HISTORY_CONTENT_CACHE_LIMIT ||
      hydratedHistoryContentCacheBytes > HYDRATED_HISTORY_CONTENT_CACHE_MAX_BYTES
    ) {
      const oldestKey = hydratedHistoryContentCache.keys().next().value as string | undefined;
      if (!oldestKey) break;
      const oldest = hydratedHistoryContentCache.get(oldestKey);
      if (oldest) hydratedHistoryContentCacheBytes -= oldest.bytes;
      hydratedHistoryContentCache.delete(oldestKey);
    }
  };


  return { readMarkdownCacheEntry, writeMarkdownCacheEntry, deleteMarkdownCacheEntry,
    readHydratedHistoryContent, writeHydratedHistoryContent };
};
