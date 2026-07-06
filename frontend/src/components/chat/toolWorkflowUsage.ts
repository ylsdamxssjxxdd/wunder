type UnknownRecord = Record<string, unknown>;
type WorkflowUsageEntryLike = {
  callItem?: unknown;
  outputItem?: unknown;
  resultItem?: unknown;
};
export type WorkflowConsumedTokenResolution = {
  tokens: number | null;
  source: 'call' | 'output' | 'result' | 'none';
};
export type WorkflowContextTokenResolution = {
  tokens: number | null;
  totalTokens: number | null;
  source: 'call' | 'output' | 'result' | 'none';
};

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

const resolveContextUsageObject = (value: unknown): UnknownRecord | null => {
  const record = asObject(value);
  if (!record) return null;
  return asObject(record.context_usage ?? record.contextUsage);
};

const resolveExplicitContextTokens = (value: unknown): number | null => {
  const record = asObject(value);
  if (!record) return null;
  const contextUsage = resolveContextUsageObject(record);
  return parsePositiveInteger(
    record.context_occupancy_tokens ??
      record.contextOccupancyTokens ??
      contextUsage?.context_occupancy_tokens ??
      contextUsage?.contextOccupancyTokens ??
      record.context_tokens ??
      record.contextTokens ??
      contextUsage?.context_tokens ??
      contextUsage?.contextTokens ??
      record.observed_context_tokens ??
      record.observedContextTokens ??
      record.observed_context_tokens_after ??
      record.observedContextTokensAfter ??
      record.final_context_tokens ??
      record.finalContextTokens ??
      record.context_tokens_after ??
      record.contextTokensAfter ??
      record.projected_request_tokens_after ??
      record.projectedRequestTokensAfter ??
      record.projected_request_tokens ??
      record.projectedRequestTokens ??
      record.persisted_context_tokens ??
      record.persistedContextTokens
  );
};

const resolveExplicitContextTotalTokens = (value: unknown): number | null => {
  const record = asObject(value);
  if (!record) return null;
  const contextUsage = resolveContextUsageObject(record);
  return parsePositiveInteger(
    record.max_context ??
      record.maxContext ??
      record.context_total_tokens ??
      record.contextTotalTokens ??
      record.context_max_tokens ??
      record.contextMaxTokens ??
      record.context_window ??
      record.contextWindow ??
      contextUsage?.max_context ??
      contextUsage?.maxContext ??
      contextUsage?.context_total_tokens ??
      contextUsage?.contextTotalTokens ??
      contextUsage?.context_max_tokens ??
      contextUsage?.contextMaxTokens ??
      contextUsage?.context_window ??
      contextUsage?.contextWindow
  );
};

type ResolveWorkflowTokensOptions = {
  allowUsageFallback?: boolean;
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
  'quota',
  'contextUsage',
  'context_usage'
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

export const resolveWorkflowConsumedTokens = (...args: unknown[]): number | null => {
  const lastArg = args[args.length - 1];
  const hasOptions =
    !!lastArg &&
    typeof lastArg === 'object' &&
    !Array.isArray(lastArg) &&
    Object.prototype.hasOwnProperty.call(lastArg, 'allowUsageFallback');
  const options = (hasOptions ? lastArg : {}) as ResolveWorkflowTokensOptions;
  const sourceArgs = hasOptions ? args.slice(0, -1) : args;
  const sources =
    sourceArgs.length === 1 && Array.isArray(sourceArgs[0]) ? (sourceArgs[0] as unknown[]) : sourceArgs;
  const candidates: UnknownRecord[] = [];
  const seen = new Set<UnknownRecord>();
  sources.forEach((source) => collectCandidateObjects(source, candidates, seen));

  for (const candidate of candidates) {
    const explicit = resolveExplicitConsumedTokens(candidate);
    if (explicit !== null) return explicit;
  }

  if (options.allowUsageFallback === false) {
    return null;
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

export const resolveWorkflowEntryConsumedTokens = (
  entry: WorkflowUsageEntryLike | null | undefined
): number | null => {
  return resolveWorkflowEntryConsumedTokenResolution(entry).tokens;
};

export const resolveWorkflowEntryConsumedTokenResolution = (
  entry: WorkflowUsageEntryLike | null | undefined
): WorkflowConsumedTokenResolution => {
  const orderedSources = [
    {
      source: 'call' as const,
      values: [entry?.callItem, asObject(entry?.callItem)?.detail],
      allowUsageFallback: true
    },
    {
      source: 'output' as const,
      values: [entry?.outputItem, asObject(entry?.outputItem)?.detail],
      allowUsageFallback: true
    },
    {
      source: 'result' as const,
      values: [entry?.resultItem, asObject(entry?.resultItem)?.detail],
      // Result payloads often carry aggregate usage snapshots for the whole user turn.
      // Per-tool row display should only trust explicit consumed-token fields here.
      allowUsageFallback: false
    }
  ];
  for (const candidate of orderedSources) {
    const sources = candidate.values;
    const resolved = resolveWorkflowConsumedTokens(sources, {
      allowUsageFallback: candidate.allowUsageFallback
    });
    if (resolved !== null) {
      return { tokens: resolved, source: candidate.source };
    }
  }
  return { tokens: null, source: 'none' };
};

export const formatWorkflowConsumedTokensLabel = (tokens: number | null): string => {
  if (tokens === null || tokens <= 0) return '';
  return `${tokens} token`;
};

export const resolveWorkflowContextTokens = (
  ...args: unknown[]
): { tokens: number | null; totalTokens: number | null } => {
  const sources = args.length === 1 && Array.isArray(args[0]) ? (args[0] as unknown[]) : args;
  const candidates: UnknownRecord[] = [];
  const seen = new Set<UnknownRecord>();
  sources.forEach((source) => collectCandidateObjects(source, candidates, seen));

  let tokens: number | null = null;
  let totalTokens: number | null = null;
  for (const candidate of candidates) {
    tokens = resolveExplicitContextTokens(candidate);
    if (tokens !== null) {
      totalTokens = resolveExplicitContextTotalTokens(candidate);
      break;
    }
  }
  if (tokens === null) {
    return { tokens: null, totalTokens: null };
  }
  if (totalTokens === null) {
    for (const candidate of candidates) {
      totalTokens = resolveExplicitContextTotalTokens(candidate);
      if (totalTokens !== null) break;
    }
  }
  return { tokens, totalTokens };
};

export const resolveWorkflowEntryContextTokenResolution = (
  entry: WorkflowUsageEntryLike | null | undefined
): WorkflowContextTokenResolution => {
  const orderedSources = [
    {
      source: 'result' as const,
      values: [entry?.resultItem, asObject(entry?.resultItem)?.detail]
    },
    {
      source: 'output' as const,
      values: [entry?.outputItem, asObject(entry?.outputItem)?.detail]
    },
    {
      source: 'call' as const,
      values: [entry?.callItem, asObject(entry?.callItem)?.detail]
    }
  ];
  let selected: WorkflowContextTokenResolution | null = null;
  let fallbackTotalTokens: number | null = null;
  for (const candidate of orderedSources) {
    const resolved = resolveWorkflowContextTokens(candidate.values);
    if (resolved.totalTokens !== null && fallbackTotalTokens === null) {
      fallbackTotalTokens = resolved.totalTokens;
    }
    if (resolved.tokens !== null && selected === null) {
      selected = {
        tokens: resolved.tokens,
        totalTokens: resolved.totalTokens,
        source: candidate.source
      };
    }
  }
  if (selected) {
    return {
      ...selected,
      totalTokens: selected.totalTokens ?? fallbackTotalTokens
    };
  }
  return { tokens: null, totalTokens: fallbackTotalTokens, source: 'none' };
};

const formatCompactTokenCount = (value: number): string => {
  const normalized = Math.max(0, Math.round(value));
  if (normalized >= 1_000_000) {
    return `${(normalized / 1_000_000).toFixed(1)}M`;
  }
  if (normalized >= 1_000) {
    return `${(normalized / 1_000).toFixed(1)}k`;
  }
  return String(normalized);
};

const formatFullTokenCount = (value: number): string => Math.max(0, Math.round(value)).toLocaleString('en-US');

export const formatWorkflowContextTokensLabel = (
  tokens: number | null,
  totalTokens: number | null,
  label: string
): string => {
  if (tokens === null || tokens <= 0) return '';
  const prefix = String(label || '').trim();
  const value = totalTokens !== null && totalTokens > 0
    ? `${formatCompactTokenCount(tokens)}/${formatCompactTokenCount(totalTokens)}`
    : formatCompactTokenCount(tokens);
  return [prefix, value].filter(Boolean).join(' ');
};

export const formatWorkflowContextTokensTitle = (
  tokens: number | null,
  totalTokens: number | null,
  label: string
): string => {
  if (tokens === null || tokens <= 0) return '';
  const prefix = String(label || '').trim();
  const value = totalTokens !== null && totalTokens > 0
    ? `${formatFullTokenCount(tokens)} / ${formatFullTokenCount(totalTokens)} token`
    : `${formatFullTokenCount(tokens)} token`;
  return [prefix, value].filter(Boolean).join(' ');
};
