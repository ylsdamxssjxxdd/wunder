type PerfSample = {
  type: 'count';
  name: string;
  value: number;
  at: number;
  meta?: Record<string, unknown>;
};

const PERF_STORAGE_KEY = 'wunder_beeroom_realtime_perf';
const MAX_SAMPLES = 300;

const perfState = {
  counters: new Map<string, number>(),
  samples: [] as PerfSample[]
};

const nowMs = (): number => Date.now();

const isEnabled = (): boolean => {
  if (typeof window === 'undefined') return false;
  const raw = String(window.localStorage.getItem(PERF_STORAGE_KEY) || '')
    .trim()
    .toLowerCase();
  return raw === '1' || raw === 'true' || raw === 'on';
};

const pushSample = (sample: PerfSample) => {
  perfState.samples.push(sample);
  if (perfState.samples.length > MAX_SAMPLES) {
    perfState.samples.shift();
  }
};

const toPlainObject = <T>(source: Map<string, T>): Record<string, T> => {
  const output: Record<string, T> = {};
  source.forEach((value, key) => {
    output[key] = value;
  });
  return output;
};

export const beeroomRealtimePerf = {
  enabled(): boolean {
    return isEnabled();
  },
  count(name: string, delta = 1, meta?: Record<string, unknown>) {
    if (!isEnabled()) return;
    const key = String(name || '').trim();
    if (!key) return;
    const value = Number.isFinite(delta) ? Number(delta) : 1;
    const current = perfState.counters.get(key) || 0;
    perfState.counters.set(key, current + value);
    pushSample({
      type: 'count',
      name: key,
      value,
      at: nowMs(),
      meta
    });
  },
  snapshot() {
    return {
      counters: toPlainObject(perfState.counters),
      samples: [...perfState.samples]
    };
  },
  reset() {
    perfState.counters.clear();
    perfState.samples = [];
  }
};

if (typeof window !== 'undefined') {
  (window as Window & { __wunderBeeroomRealtimePerf?: typeof beeroomRealtimePerf }).__wunderBeeroomRealtimePerf =
    beeroomRealtimePerf;
}
