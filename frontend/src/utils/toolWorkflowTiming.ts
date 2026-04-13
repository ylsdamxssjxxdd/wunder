type UnknownRecord = Record<string, unknown>;

type WorkflowTimingEntryLike = {
  callItem?: unknown;
  outputItem?: unknown;
  resultItem?: unknown;
};

const DIRECT_TIMING_KEYS = [
  'duration_ms',
  'elapsed_ms',
  'durationMs',
  'elapsedMs',
  'latency_ms',
  'latencyMs'
] as const;

const NESTED_TIMING_KEYS = [
  'data',
  'result',
  'meta',
  'summary',
  'payload',
  'output_meta',
  'outputMeta',
  'timing',
  'timings',
  'stats',
  'performance',
  'perf',
  'search'
] as const;

const MAX_SCAN_DEPTH = 4;
const MAX_ARRAY_SCAN = 16;
const MAX_OBJECT_SCAN = 40;

const asRecord = (value: unknown): UnknownRecord | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  return value as UnknownRecord;
};

const parseNumericDuration = (value: unknown): number | null => {
  if (typeof value === 'number' && Number.isFinite(value) && value > 0) {
    return Math.trunc(value);
  }
  if (typeof value !== 'string') {
    return null;
  }
  const normalized = value.trim();
  if (!/^\d+(?:\.\d+)?$/.test(normalized)) {
    return null;
  }
  const parsed = Number(normalized);
  return Number.isFinite(parsed) && parsed > 0 ? Math.trunc(parsed) : null;
};

const parseStructuredValue = (value: unknown): unknown => {
  if (typeof value !== 'string') {
    return value;
  }
  const normalized = value.trim();
  if (!normalized) {
    return value;
  }
  if (!normalized.startsWith('{') && !normalized.startsWith('[')) {
    return value;
  }
  try {
    return JSON.parse(normalized);
  } catch {
    return value;
  }
};

const resolveDirectDurationMs = (record: UnknownRecord): number | null => {
  for (const key of DIRECT_TIMING_KEYS) {
    const durationMs = parseNumericDuration(record[key]);
    if (durationMs !== null) {
      return durationMs;
    }
  }
  return null;
};

const resolveDurationFromValue = (
  value: unknown,
  depth: number,
  seen: Set<UnknownRecord>
): number | null => {
  if (depth > MAX_SCAN_DEPTH || value === null || value === undefined) {
    return null;
  }

  const scalarDuration = parseNumericDuration(value);
  if (scalarDuration !== null) {
    return scalarDuration;
  }

  const structured = parseStructuredValue(value);
  if (structured !== value) {
    return resolveDurationFromValue(structured, depth, seen);
  }

  if (Array.isArray(structured)) {
    for (const item of structured.slice(0, MAX_ARRAY_SCAN)) {
      const durationMs = resolveDurationFromValue(item, depth + 1, seen);
      if (durationMs !== null) {
        return durationMs;
      }
    }
    return null;
  }

  const record = asRecord(structured);
  if (!record || seen.has(record)) {
    return null;
  }
  seen.add(record);

  const directDuration = resolveDirectDurationMs(record);
  if (directDuration !== null) {
    return directDuration;
  }

  for (const key of NESTED_TIMING_KEYS) {
    const durationMs = resolveDurationFromValue(record[key], depth + 1, seen);
    if (durationMs !== null) {
      return durationMs;
    }
  }

  const entries = Object.entries(record);
  for (const [, nestedValue] of entries.slice(0, MAX_OBJECT_SCAN)) {
    const durationMs = resolveDurationFromValue(nestedValue, depth + 1, seen);
    if (durationMs !== null) {
      return durationMs;
    }
  }

  return null;
};

export const resolveWorkflowDurationMs = (...args: unknown[]): number | null => {
  const sources =
    args.length === 1 && Array.isArray(args[0]) ? (args[0] as unknown[]) : args;
  const seen = new Set<UnknownRecord>();
  for (const source of sources) {
    const durationMs = resolveDurationFromValue(source, 0, seen);
    if (durationMs !== null) {
      return durationMs;
    }
  }
  return null;
};

export const resolveWorkflowEntryDurationMs = (
  entry: WorkflowTimingEntryLike | null | undefined,
  liveDurationMs: number | null = null
): number | null => {
  if (liveDurationMs !== null && liveDurationMs > 0) {
    return liveDurationMs;
  }
  return resolveWorkflowDurationMs(
    entry?.resultItem,
    asRecord(entry?.resultItem)?.detail,
    entry?.outputItem,
    asRecord(entry?.outputItem)?.detail,
    entry?.callItem,
    asRecord(entry?.callItem)?.detail
  );
};

export const formatWorkflowDurationLabel = (durationMs: number | null): string => {
  if (durationMs === null || durationMs <= 0) return '';
  if (durationMs < 1000) return `${durationMs}ms`;
  const seconds = durationMs / 1000;
  return seconds >= 10 ? `${seconds.toFixed(0)}s` : `${seconds.toFixed(1)}s`;
};
