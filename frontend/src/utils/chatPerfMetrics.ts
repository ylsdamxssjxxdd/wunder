// Fixed buckets avoid retaining one object per token/frame. Percentiles are upper bounds.
export const PERF_BUCKETS = [8, 16, 24, 32, 50, 100, 200, 500, 1000, 3000, 10000];
const METRICS = new Set([
  'browser_frame_gap', 'browser_long_task', 'browser_input_event',
  'chat_stream_plain_text_flush', 'chat_stream_content_clock_flush',
  'chat_stream_markdown_render', 'chat_workflow_entries_build', 'chat_snapshot_flush',
  'chat_session_detail_load', 'chat_history_load', 'chat_stream_total', 'chat_resume_total',
  'chat_stream_event', 'chat_resume_event', 'chat_watch_event', 'chat_shell_render',
  'chat_message_panel_render', 'chat_stream_interrupted', 'chat_resume_interrupted',
  'chat_watch_interrupted', 'chat_history_load_failed', 'chat_watch_terminal',
  'chat_watchdog_idle_complete', 'chat_watchdog_idle', 'chat_slow_client_auto_resume',
  'workspace.panel.refresh', 'workspace.panel.refresh.event',
  'workspace.panel.refresh.settle.ms', 'workspace.panel.refresh.incremental.ms',
  'workspace.panel.refresh.full.ms'
]);
const META_KEYS = new Set(['contentLength', 'messageCount', 'itemCount', 'fetchMs',
  'hydrateMs', 'foregroundSyncMs', 'transcriptCount', 'eventCount', 'roundCount']);
type Duration = { count: number; totalMs: number; maxMs: number; buckets: number[] };
type SlowSample = { atMs: number; metric: string; durationMs: number; sizes?: Record<string, number> };
export type PerfMetricsState = {
  counters: Record<string, number>;
  durations: Record<string, Duration>;
  slowest: SlowSample[];
};
export const emptyPerfMetrics = (): PerfMetricsState => ({ counters: {}, durations: {}, slowest: [] });
const normalize = (name: string) => name.replace('_slow_flush', '_flush').replace('_slow_render', '_render');
const round = (value: number) => Math.round(value * 10) / 10;

export const incrementPerfCounter = (state: PerfMetricsState, name: string, delta = 1) => {
  if (!METRICS.has(name) || !Number.isFinite(delta) || delta <= 0) return;
  state.counters[name] = (state.counters[name] || 0) + delta;
};

export const recordPerfDuration = (state: PerfMetricsState, rawName: string, durationMs: number,
  atMs: number, meta?: Record<string, unknown>) => {
  const name = normalize(rawName);
  if (!METRICS.has(name) || !Number.isFinite(durationMs) || durationMs < 0) return;
  const value = state.durations[name] ||= { count: 0, totalMs: 0, maxMs: 0,
    buckets: Array(PERF_BUCKETS.length + 1).fill(0) };
  value.count++;
  value.totalMs += durationMs;
  value.maxMs = Math.max(value.maxMs, durationMs);
  const bucket = PERF_BUCKETS.findIndex(limit => durationMs <= limit);
  value.buckets[bucket < 0 ? PERF_BUCKETS.length : bucket]++;
  // Network/model wall times are useful aggregates, but not main-thread stalls.
  if (durationMs < 50 || ['browser_frame_gap', 'chat_stream_total', 'chat_resume_total'].includes(name)) return;
  if (state.slowest.length >= 30 && durationMs <= state.slowest[state.slowest.length - 1].durationMs) return;
  const sizes: Record<string, number> = {};
  for (const key of META_KEYS) {
    const number = meta?.[key];
    if (typeof number === 'number' && Number.isFinite(number)) sizes[key] = round(number);
  }
  state.slowest.push({ atMs: round(atMs), metric: name, durationMs: round(durationMs),
    ...(Object.keys(sizes).length ? { sizes } : {}) });
  state.slowest.sort((a, b) => b.durationMs - a.durationMs);
  state.slowest.length = Math.min(state.slowest.length, 30);
};

export const summarizePerfMetrics = (state: PerfMetricsState) => ({
  counters: { ...state.counters },
  durations: Object.fromEntries(Object.entries(state.durations).map(([name, value]) => {
    let accumulated = 0;
    const index = value.buckets.findIndex(count => (accumulated += count) >= Math.ceil(value.count * 0.95));
    return [name, { count: value.count, avgMs: round(value.totalMs / value.count),
      maxMs: round(value.maxMs), totalMs: round(value.totalMs),
      p95UpperMs: PERF_BUCKETS[index] ?? round(value.maxMs) }];
  })),
  slowest: state.slowest.map(sample => ({ ...sample, ...(sample.sizes ? { sizes: { ...sample.sizes } } : {}) }))
});
