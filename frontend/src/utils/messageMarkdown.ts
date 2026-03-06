type WorkflowLikeItem = {
  detail?: unknown;
};

type MessageLike = {
  workflowItems?: WorkflowLikeItem[] | null;
};

const EXTERNAL_IMAGE_URL_REGEX = /https?:\/\/[^\s<>"'`]+/gi;
const MARKDOWN_IMAGE_REGEX = /!\[([^\]]*)\]\((https?:\/\/[^\s)]+)\)/g;
const IMAGE_URL_SUFFIX_REGEX = /\.(?:png|jpe?g|gif|webp|svg)(?:$|[?#])/i;

const stringifyValue = (value: unknown): string => {
  if (typeof value === 'string') return value;
  if (value === null || value === undefined) return '';
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
};

const normalizeUrlCandidate = (value: string): string => {
  const trimmed = String(value || '').trim().replace(/[),.;]+$/u, '');
  if (!trimmed || !/^https?:\/\//i.test(trimmed)) return '';
  if (!IMAGE_URL_SUFFIX_REGEX.test(trimmed)) return '';
  return trimmed;
};

const buildImageUrlKey = (value: string): string => {
  const normalized = normalizeUrlCandidate(value);
  if (!normalized) return '';
  try {
    const parsed = new URL(normalized);
    return `${parsed.origin}${parsed.pathname}`.toLowerCase();
  } catch {
    return normalized.split('?')[0].toLowerCase();
  }
};

const resolveUrlExpires = (value: string): number => {
  try {
    const parsed = new URL(value);
    const expires = Number(parsed.searchParams.get('Expires'));
    return Number.isFinite(expires) ? expires : -1;
  } catch {
    return -1;
  }
};

const shouldPreferImageUrl = (candidate: string, current: string): boolean => {
  const candidateExpires = resolveUrlExpires(candidate);
  const currentExpires = resolveUrlExpires(current);
  if (candidateExpires !== currentExpires) {
    return candidateExpires > currentExpires;
  }
  return candidate.length > current.length;
};

const extractImageUrls = (value: unknown): string[] => {
  const source = stringifyValue(value);
  if (!source) return [];
  const matches = source.match(EXTERNAL_IMAGE_URL_REGEX) || [];
  const urls: string[] = [];
  const seen = new Set<string>();
  matches.forEach((match) => {
    const normalized = normalizeUrlCandidate(match);
    if (!normalized || seen.has(normalized)) return;
    seen.add(normalized);
    urls.push(normalized);
  });
  return urls;
};

export const collectKnownImageUrlsFromMessage = (message: MessageLike | null | undefined): string[] => {
  if (!message || !Array.isArray(message.workflowItems) || message.workflowItems.length === 0) {
    return [];
  }
  const preferredByKey = new Map<string, string>();
  message.workflowItems.forEach((item) => {
    extractImageUrls(item?.detail).forEach((url) => {
      const key = buildImageUrlKey(url);
      if (!key) return;
      const current = preferredByKey.get(key);
      if (!current || shouldPreferImageUrl(url, current)) {
        preferredByKey.set(key, url);
      }
    });
  });
  return [...preferredByKey.values()];
};

const repairMarkdownImageUrls = (content: string, knownImageUrls: string[]): string => {
  if (!content || knownImageUrls.length === 0) return content;
  const preferredByKey = new Map<string, string>();
  knownImageUrls.forEach((url) => {
    const key = buildImageUrlKey(url);
    if (!key) return;
    const current = preferredByKey.get(key);
    if (!current || shouldPreferImageUrl(url, current)) {
      preferredByKey.set(key, url);
    }
  });
  if (preferredByKey.size === 0) return content;
  return content.replace(MARKDOWN_IMAGE_REGEX, (match, alt, url) => {
    const key = buildImageUrlKey(url);
    const repaired = key ? preferredByKey.get(key) : '';
    if (!repaired || repaired === url) return match;
    return `![${alt}](${repaired})`;
  });
};

export const prepareMessageMarkdownContent = (
  content: unknown,
  message: MessageLike | null | undefined
): string => {
  const source = String(content || '');
  if (!source) return '';
  return repairMarkdownImageUrls(source, collectKnownImageUrlsFromMessage(message));
};
