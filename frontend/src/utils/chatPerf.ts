type PerfSample = {
  ts: number;
  kind: string;
  sessionId?: string;
  durationMs?: number;
  meta?: Record<string, unknown>;
};

type PerfDuration = {
  count: number;
  totalMs: number;
  maxMs: number;
  minMs: number;
};

const PERF_STORAGE_KEY = 'wunder_chat_perf';
const MAX_SAMPLES = 2000;

const perfState = {
  samples: [] as PerfSample[],
  counters: new Map<string, number>(),
  durations: new Map<string, PerfDuration>()
};

const isEnabled = (): boolean => {
  try {
    return typeof localStorage !== 'undefined' && localStorage.getItem(PERF_STORAGE_KEY) === '1';
  } catch {
    return false;
  }
};

const pushSample = (sample: PerfSample) => {
  perfState.samples.push(sample);
  if (perfState.samples.length > MAX_SAMPLES) {
    perfState.samples.shift();
  }
};

const bumpCounter = (name: string, delta = 1) => {
  const current = perfState.counters.get(name) || 0;
  perfState.counters.set(name, current + delta);
};

const recordDuration = (name: string, durationMs: number, meta?: Record<string, unknown>) => {
  if (!Number.isFinite(durationMs) || durationMs < 0) return;
  const current =
    perfState.durations.get(name) || {
      count: 0,
      totalMs: 0,
      maxMs: 0,
      minMs: Number.POSITIVE_INFINITY
    };
  current.count += 1;
  current.totalMs += durationMs;
  current.maxMs = Math.max(current.maxMs, durationMs);
  current.minMs = Math.min(current.minMs, durationMs);
  perfState.durations.set(name, current);
  pushSample({
    ts: Date.now(),
    kind: name,
    durationMs,
    meta
  });
};

const toPlainObject = <T>(map: Map<string, T>): Record<string, T> => {
  const output: Record<string, T> = {};
  map.forEach((value, key) => {
    output[key] = value;
  });
  return output;
};

export const chatPerf = {
  enabled: isEnabled,
  count: (name: string, delta = 1, meta?: Record<string, unknown>) => {
    if (!isEnabled()) return;
    bumpCounter(name, delta);
    pushSample({
      ts: Date.now(),
      kind: name,
      meta
    });
  },
  recordDuration: (name: string, durationMs: number, meta?: Record<string, unknown>) => {
    if (!isEnabled()) return;
    recordDuration(name, durationMs, meta);
  },
  time: <T>(name: string, fn: () => T, meta?: Record<string, unknown>): T => {
    if (!isEnabled()) return fn();
    const start = performance.now();
    const result = fn();
    recordDuration(name, performance.now() - start, meta);
    return result;
  },
  snapshot: () => ({
    enabled: isEnabled(),
    counters: toPlainObject(perfState.counters),
    durations: toPlainObject(perfState.durations),
    samples: [...perfState.samples]
  }),
  reset: () => {
    perfState.samples = [];
    perfState.counters.clear();
    perfState.durations.clear();
  }
};

if (typeof window !== 'undefined') {
  (window as Window & { __wunderChatPerf?: typeof chatPerf }).__wunderChatPerf = chatPerf;
}
