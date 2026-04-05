const DETAIL_DISPLAY_CACHE_LIMIT = 120;

const detailDisplayCache = new Map<string, string>();

const getCachedDisplay = (raw: string): string | null => {
  const cached = detailDisplayCache.get(raw);
  if (!cached) return null;
  detailDisplayCache.delete(raw);
  detailDisplayCache.set(raw, cached);
  return cached;
};

const setCachedDisplay = (raw: string, display: string): void => {
  detailDisplayCache.set(raw, display);
  if (detailDisplayCache.size <= DETAIL_DISPLAY_CACHE_LIMIT) return;
  const oldest = detailDisplayCache.keys().next().value as string | undefined;
  if (oldest) detailDisplayCache.delete(oldest);
};

const normalizeNewlines = (value: string): string =>
  value.replace(/\r\n/g, '\n').replace(/\r/g, '\n');

const tryParseJson = (value: string): { ok: true; parsed: unknown } | { ok: false } => {
  try {
    return { ok: true, parsed: JSON.parse(value) };
  } catch {
    return { ok: false };
  }
};

const normalizeJsonLine = (line: string): string | null => {
  const trimmed = line.trim();
  if (!trimmed) return null;
  const candidate = trimmed.startsWith('data:') ? trimmed.slice(5).trim() : trimmed;
  if (!candidate || candidate === '[DONE]') return null;
  return candidate;
};

const tryFormatJsonLines = (raw: string): string | null => {
  const lines = normalizeNewlines(raw).split('\n');
  const parsedRows: unknown[] = [];
  let seenJsonLine = false;
  for (const line of lines) {
    const candidate = normalizeJsonLine(line);
    if (!candidate) continue;
    seenJsonLine = true;
    const parsed = tryParseJson(candidate);
    if (!parsed.ok) return null;
    parsedRows.push(parsed.parsed);
  }
  if (!seenJsonLine || parsedRows.length === 0) return null;
  return JSON.stringify(parsedRows, null, 2);
};

export const formatWorkflowDetailForDisplay = (rawDetail: string): string => {
  if (!rawDetail) return '';
  const cached = getCachedDisplay(rawDetail);
  if (cached !== null) return cached;

  const trimmed = rawDetail.trim();
  if (!trimmed) {
    setCachedDisplay(rawDetail, rawDetail);
    return rawDetail;
  }

  const parsedJson = tryParseJson(trimmed);
  if (parsedJson.ok) {
    const display = JSON.stringify(parsedJson.parsed, null, 2);
    setCachedDisplay(rawDetail, display);
    return display;
  }

  const lineFormatted = tryFormatJsonLines(rawDetail);
  if (lineFormatted) {
    setCachedDisplay(rawDetail, lineFormatted);
    return lineFormatted;
  }

  setCachedDisplay(rawDetail, rawDetail);
  return rawDetail;
};
