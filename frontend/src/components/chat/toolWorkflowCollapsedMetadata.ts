import type { RawToolRun, WorkflowItem } from './toolWorkflowRunModel';

type UnknownRecord = Record<string, unknown>;
type MetadataSource = 'call' | 'output' | 'result' | 'none';

export type CollapsedWorkflowEntryMetadata = {
  contextTokensLabel: string;
  contextTokensSource: MetadataSource;
  consumedTokensLabel: string;
  consumedTokensSource: MetadataSource;
  durationLabel: string;
};

const RAW_SCAN_LIMIT = 8_192;
const MAX_RECORD_DEPTH = 4;
const MAX_RECORD_COUNT = 32;
const EXPLICIT_CONSUMED_KEYS = [
  'request_consumed_tokens',
  'requestConsumedTokens',
  'consumed_tokens',
  'consumedTokens',
  'consumed',
  'used',
  'count'
] as const;
const USAGE_CONTAINER_KEYS = [
  'roundUsage',
  'round_usage',
  'billedUsage',
  'billed_usage',
  'usage'
] as const;
const USAGE_VALUE_KEYS = ['total', 'total_tokens', 'totalTokens', 'input', 'input_tokens', 'inputTokens'] as const;
const CONTEXT_TOKEN_KEYS = [
  'context_occupancy_tokens',
  'contextOccupancyTokens',
  'context_tokens',
  'contextTokens',
  'observed_context_tokens',
  'observedContextTokens',
  'observed_context_tokens_after',
  'observedContextTokensAfter',
  'final_context_tokens',
  'finalContextTokens',
  'context_tokens_after',
  'contextTokensAfter',
  'projected_request_tokens_after',
  'projectedRequestTokensAfter',
  'projected_request_tokens',
  'projectedRequestTokens',
  'persisted_context_tokens',
  'persistedContextTokens'
] as const;
const DURATION_KEYS = [
  'duration_ms',
  'elapsed_ms',
  'durationMs',
  'elapsedMs',
  'latency_ms',
  'latencyMs'
] as const;
const RAW_METADATA_KEYS = [
  'detail',
  'toolCallRawDetail',
  'tool_call_raw_detail',
  'toolResultRawDetail',
  'tool_result_raw_detail',
  'payload',
  'data',
  'result',
  'meta'
] as const;

const asRecord = (value: unknown): UnknownRecord | null =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? value as UnknownRecord
    : null;

const parsePositiveInteger = (value: unknown): number | null => {
  if (typeof value === 'number' && Number.isFinite(value) && value > 0) {
    return Math.floor(value);
  }
  if (typeof value !== 'string' || !/^\d+$/.test(value.trim())) return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const compactRawSample = (value: string): string => {
  if (value.length <= RAW_SCAN_LIMIT) return value;
  const sideLimit = Math.floor(RAW_SCAN_LIMIT / 2);
  return `${value.slice(0, sideLimit)}\n${value.slice(-sideLimit)}`;
};

const collectRecords = (value: unknown): UnknownRecord[] => {
  const root = asRecord(value);
  if (!root) return [];
  const records: UnknownRecord[] = [];
  const seen = new Set<UnknownRecord>();
  const pending: Array<{ record: UnknownRecord; depth: number }> = [{ record: root, depth: 0 }];
  while (pending.length > 0 && records.length < MAX_RECORD_COUNT) {
    const current = pending.shift();
    if (!current || seen.has(current.record)) continue;
    seen.add(current.record);
    records.push(current.record);
    if (current.depth >= MAX_RECORD_DEPTH) continue;
    Object.values(current.record).forEach((child) => {
      const childRecord = asRecord(child);
      if (childRecord && !seen.has(childRecord)) {
        pending.push({ record: childRecord, depth: current.depth + 1 });
      }
    });
  }
  return records;
};

const collectRawMetadata = (records: UnknownRecord[]): string[] => {
  const values: string[] = [];
  records.forEach((record) => {
    RAW_METADATA_KEYS.forEach((key) => {
      const value = record[key];
      if (typeof value !== 'string' || !value.trim()) return;
      const sample = compactRawSample(value);
      if (!values.includes(sample)) values.push(sample);
    });
  });
  return values;
};

const escapeRegex = (value: string): string => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

const readRawNumber = (values: string[], keys: readonly string[]): number | null => {
  for (const value of values) {
    for (const key of keys) {
      const match = new RegExp(`"${escapeRegex(key)}"\\s*:\\s*"?(\\d+)"?`, 'i').exec(value);
      const parsed = parsePositiveInteger(match?.[1]);
      if (parsed !== null) return parsed;
    }
  }
  return null;
};

const readNumber = (
  records: UnknownRecord[],
  rawValues: string[],
  keys: readonly string[]
): number | null => {
  for (const record of records) {
    for (const key of keys) {
      const parsed = parsePositiveInteger(record[key]);
      if (parsed !== null) return parsed;
    }
  }
  return readRawNumber(rawValues, keys);
};

const readUsageFallback = (records: UnknownRecord[], rawValues: string[]): number | null => {
  for (const record of records) {
    for (const containerKey of USAGE_CONTAINER_KEYS) {
      const usage = asRecord(record[containerKey]);
      if (!usage) continue;
      for (const valueKey of USAGE_VALUE_KEYS) {
        const parsed = parsePositiveInteger(usage[valueKey]);
        if (parsed !== null) return parsed;
      }
    }
  }
  for (const rawValue of rawValues) {
    for (const containerKey of USAGE_CONTAINER_KEYS) {
      const containerMatch = new RegExp(
        `"${escapeRegex(containerKey)}"\\s*:\\s*\\{([\\s\\S]{0,2048}?)\\}`,
        'i'
      ).exec(rawValue);
      if (!containerMatch?.[1]) continue;
      const parsed = readRawNumber([containerMatch[1]], USAGE_VALUE_KEYS);
      if (parsed !== null) return parsed;
    }
  }
  return null;
};

const resolveItemMetadata = (item: WorkflowItem | null): { records: UnknownRecord[]; rawValues: string[] } => {
  const records = collectRecords(item);
  return { records, rawValues: collectRawMetadata(records) };
};

const formatTokenLabel = (tokens: number | null): string => tokens === null ? '' : `${tokens} token`;

const formatDurationLabel = (durationMs: number | null): string => {
  if (durationMs === null) return '';
  if (durationMs < 1000) return `${durationMs}ms`;
  const seconds = durationMs / 1000;
  return seconds >= 10 ? `${seconds.toFixed(0)}s` : `${seconds.toFixed(1)}s`;
};

// Collapsed rows must never JSON-parse full tool results. This bounded scan keeps
// token and duration metadata visible without moving large result formatting back
// into the scrolling hot path.
export const resolveCollapsedWorkflowEntryMetadata = (
  entry: RawToolRun,
  liveDurationMs: number | null = null
): CollapsedWorkflowEntryMetadata => {
  const sources: Array<{ source: Exclude<MetadataSource, 'none'>; item: WorkflowItem | null }> = [
    { source: 'call', item: entry.callItem },
    { source: 'output', item: entry.outputItem },
    { source: 'result', item: entry.resultItem }
  ];
  const metadataBySource = sources.map((candidate) => ({
    source: candidate.source,
    ...resolveItemMetadata(candidate.item)
  }));

  let contextTokens: number | null = null;
  let contextTokensSource: MetadataSource = 'none';
  let consumedTokens: number | null = null;
  let consumedTokensSource: MetadataSource = 'none';
  for (const metadata of metadataBySource) {
    if (contextTokens === null) {
      const resolved = readNumber(metadata.records, metadata.rawValues, CONTEXT_TOKEN_KEYS);
      if (resolved !== null) {
        contextTokens = resolved;
        contextTokensSource = metadata.source;
      }
    }
    if (consumedTokens === null) {
      const explicit = readNumber(metadata.records, metadata.rawValues, EXPLICIT_CONSUMED_KEYS);
      const fallback = metadata.source === 'result'
        ? null
        : readUsageFallback(metadata.records, metadata.rawValues);
      const resolved = explicit ?? fallback;
      if (resolved !== null) {
        consumedTokens = resolved;
        consumedTokensSource = metadata.source;
      }
    }
  }

  let durationMs = parsePositiveInteger(liveDurationMs);
  if (durationMs === null) {
    for (const metadata of [...metadataBySource].reverse()) {
      durationMs = readNumber(metadata.records, metadata.rawValues, DURATION_KEYS);
      if (durationMs !== null) break;
    }
  }
  return {
    contextTokensLabel: formatTokenLabel(contextTokens),
    contextTokensSource,
    consumedTokensLabel: formatTokenLabel(consumedTokens),
    consumedTokensSource,
    durationLabel: formatDurationLabel(durationMs)
  };
};
