type UnknownRecord = Record<string, unknown>;

const asObject = (value: unknown): UnknownRecord | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  return value as UnknownRecord;
};

const parseObject = (value: unknown): UnknownRecord | null => {
  const direct = asObject(value);
  if (direct) return direct;
  if (typeof value !== 'string') return null;
  const normalized = value.trim();
  if (!normalized) return null;
  try {
    return asObject(JSON.parse(normalized));
  } catch {
    return null;
  }
};

const parsePositiveInteger = (value: unknown): number | null => {
  if (value === null || value === undefined || value === '') return null;
  const parsed = Number.parseInt(String(value), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const resolveUsageConsumedTokens = (value: unknown): number | null => {
  const record = asObject(value);
  if (!record) return null;
  return (
    parsePositiveInteger(record.total ?? record.total_tokens ?? record.totalTokens) ??
    parsePositiveInteger(record.input ?? record.input_tokens ?? record.inputTokens)
  );
};

const resolveQuotaConsumedTokens = (value: unknown): number | null => {
  const record = asObject(value);
  if (!record) {
    return parsePositiveInteger(value);
  }
  return (
    parsePositiveInteger(
      record.request_consumed_tokens ??
        record.requestConsumedTokens ??
        record.consumed_tokens ??
        record.consumedTokens ??
        record.consumed ??
        record.used ??
        record.count
    ) ?? null
  );
};

const resolveExplicitConsumedTokens = (value: unknown): number | null => {
  const record = asObject(value);
  if (!record) return null;
  return (
    parsePositiveInteger(
      record.request_consumed_tokens ??
        record.requestConsumedTokens ??
        record.consumed_tokens ??
        record.consumedTokens
    ) ??
    resolveQuotaConsumedTokens(record.quotaConsumed ?? record.quota_consumed ?? record.quota)
  );
};

const NESTED_USAGE_KEYS = [
  'data',
  'result',
  'meta',
  'payload',
  'stats',
  'usage',
  'roundUsage',
  'round_usage',
  'billedUsage',
  'billed_usage',
  'quotaConsumed',
  'quota_consumed',
  'quota'
] as const;

const collectCandidateObjects = (
  value: unknown,
  output: UnknownRecord[],
  seen: Set<UnknownRecord>,
  depth = 0
): void => {
  if (depth > 3) return;
  const record = parseObject(value);
  if (!record || seen.has(record)) return;
  seen.add(record);
  output.push(record);
  NESTED_USAGE_KEYS.forEach((key) => {
    collectCandidateObjects(record[key], output, seen, depth + 1);
  });
};

export const resolveWorkflowConsumedTokens = (...sources: unknown[]): number | null => {
  const candidates: UnknownRecord[] = [];
  const seen = new Set<UnknownRecord>();
  sources.forEach((source) => collectCandidateObjects(source, candidates, seen));

  for (const candidate of candidates) {
    const explicit = resolveExplicitConsumedTokens(candidate);
    if (explicit !== null) return explicit;
  }

  for (const candidate of candidates) {
    const fallback =
      resolveUsageConsumedTokens(
        candidate.roundUsage ??
          candidate.round_usage ??
          candidate.billedUsage ??
          candidate.billed_usage
      ) ?? resolveUsageConsumedTokens(candidate.usage);
    if (fallback !== null) return fallback;
  }

  return null;
};

export const formatWorkflowConsumedTokensLabel = (tokens: number | null): string => {
  if (tokens === null || tokens <= 0) return '';
  return `${tokens.toLocaleString('en-US')} tok`;
};
