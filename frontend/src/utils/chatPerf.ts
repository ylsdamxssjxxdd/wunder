import { emptyPerfMetrics, incrementPerfCounter, recordPerfDuration, summarizePerfMetrics } from './chatPerfMetrics';
import { observeChatBrowserPerformance } from './chatPerfBrowser';

const STORAGE_KEY = 'wunder:performance-capture';
const MAX_CAPTURE_MS = 30 * 60 * 1000;
const listeners = new Set<() => void>();
const freshCapture = () => ({
  schemaVersion: 2 as const, startedAt: 0, stoppedAt: 0, pages: 1,
  metrics: emptyPerfMetrics(),
  frames: { count: 0, visibleMs: 0, over50Ms: 0, over100Ms: 0 },
  peaks: { elements: 0, messages: 0, tools: 0, heapBytes: null as number | null },
  capabilities: { longTask: false, inputEvent: false, heap: false }
});
let capture = freshCapture();
let running = false;
let browser: ReturnType<typeof observeChatBrowserPerformance> | null = null;
let expiry: ReturnType<typeof setTimeout> | undefined;
const status = () => ({ running, hasReport: capture.startedAt > 0 });
const notify = () => listeners.forEach(listener => listener());
const persist = () => {
  try { sessionStorage.setItem(STORAGE_KEY, JSON.stringify(capture)); } catch { /* In-memory capture remains usable. */ }
};
const stopObservers = () => {
  browser?.dispose();
  browser = null;
  clearTimeout(expiry);
};
const recordDuration = (name: string, ms: number, meta?: Record<string, unknown>) => {
  if (running) recordPerfDuration(capture.metrics, name, ms, Date.now() - capture.startedAt, meta);
};
const activate = () => {
  running = true;
  if (typeof window !== 'undefined' && typeof requestAnimationFrame === 'function') {
    browser = observeChatBrowserPerformance({
      duration: recordDuration,
      frame: ms => {
        capture.frames.count++;
        capture.frames.visibleMs += ms;
        if (ms > 50) capture.frames.over50Ms++;
        if (ms > 100) capture.frames.over100Ms++;
        recordDuration('browser_frame_gap', ms);
      },
      dom: sample => {
        for (const key of ['elements', 'messages', 'tools'] as const) capture.peaks[key] = Math.max(capture.peaks[key], sample[key]);
        if (sample.heapBytes !== null) capture.peaks.heapBytes = Math.max(capture.peaks.heapBytes || 0, sample.heapBytes);
      }
    });
    capture.capabilities = browser.capabilities;
    expiry = setTimeout(() => chatPerf.stop(), Math.max(0, MAX_CAPTURE_MS - (Date.now() - capture.startedAt)));
  }
  notify();
};
const snapshot = () => {
  const metrics = summarizePerfMetrics(capture.metrics);
  const duration = (name: string) => metrics.durations[name] ?? null;
  return {
    schemaVersion: 2,
    enabled: running,
    capture: { startedAt: capture.startedAt ? new Date(capture.startedAt).toISOString() : null,
      stoppedAt: capture.stoppedAt ? new Date(capture.stoppedAt).toISOString() : null,
      elapsedMs: capture.startedAt ? (capture.stoppedAt || Date.now()) - capture.startedAt : 0,
      pages: capture.pages },
    summary: {
      responsiveness: { frameGaps: duration('browser_frame_gap'), gapsOver50Ms: capture.frames.over50Ms,
        gapsOver100Ms: capture.frames.over100Ms, visibleMs: Math.round(capture.frames.visibleMs),
        longTasks: capture.capabilities.longTask ? duration('browser_long_task') : null,
        slowInputEvents: capture.capabilities.inputEvent ? duration('browser_input_event') : null },
      rendering: { shellUpdates: metrics.counters.chat_shell_render || 0,
        messagePanelUpdates: metrics.counters.chat_message_panel_render || 0,
        textFlush: duration('chat_stream_plain_text_flush'),
        markdownWorkerWallTime: duration('chat_stream_markdown_render'),
        workflowBuild: duration('chat_workflow_entries_build') },
      loading: { session: duration('chat_session_detail_load'), history: duration('chat_history_load'),
        snapshot: duration('chat_snapshot_flush') },
      sampledPeaks: { ...capture.peaks }
    },
    environment: { appVersion: typeof __WUNDER_APP_VERSION__ !== 'undefined' ? __WUNDER_APP_VERSION__ : 'dev',
      browser: typeof navigator !== 'undefined' ? navigator.userAgent : null,
      logicalProcessors: typeof navigator !== 'undefined' ? navigator.hardwareConcurrency : null,
      viewport: typeof window !== 'undefined' ? { width: innerWidth, height: innerHeight, dpr: devicePixelRatio } : null,
      capabilities: { ...capture.capabilities } },
    notes: ['p95UpperMs is a histogram upper bound; DOM/heap peaks are sampled every 3 seconds.',
      'Frame gaps exclude hidden tabs. Input events include only durations >=40ms; this is not INP.',
      'Markdown worker and session/stream durations include waiting, not main-thread CPU time.',
      'No message text, identifiers, paths, URLs or debug payloads are collected.'],
    ...metrics
  };
};

export const chatPerf = {
  enabled: () => running,
  status,
  subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
  start: () => {
    if (running) return status();
    capture = freshCapture();
    capture.startedAt = Date.now();
    activate();
    persist();
    return status();
  },
  stop: () => {
    if (!running) return status();
    running = false;
    capture.stoppedAt = Date.now();
    stopObservers();
    persist();
    notify();
    return status();
  },
  count: (name: string, delta = 1, _meta?: Record<string, unknown>) => {
    if (running) incrementPerfCounter(capture.metrics, name, delta);
  },
  recordDuration,
  time: <T>(name: string, fn: () => T, meta?: Record<string, unknown>): T => {
    if (!running) return fn();
    const start = performance.now();
    try { return fn(); } finally { recordDuration(name, performance.now() - start, meta); }
  },
  snapshot,
  download: () => {
    if (!capture.startedAt) return false;
    const url = URL.createObjectURL(new Blob([JSON.stringify(snapshot(), null, 2)], { type: 'application/json' }));
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = `wunder-performance-${new Date().toISOString().replace(/[:.]/g, '-')}.json`;
    try { document.body.appendChild(anchor); anchor.click(); }
    finally { anchor.remove(); setTimeout(() => URL.revokeObjectURL(url), 1000); }
    return true;
  }
};

if (typeof window !== 'undefined') {
  // Tab-scoped state survives refresh without enabling capture in every open tab.
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY);
    const stored = raw && raw.length < 64000 ? JSON.parse(raw) : null;
    const finite = (value: unknown) => typeof value === 'number' && Number.isFinite(value) && value >= 0;
    const validMetrics = stored?.metrics && stored.metrics.counters && stored.metrics.durations &&
      Object.keys(stored.metrics.counters).length <= 40 && Object.keys(stored.metrics.durations).length <= 40 &&
      Object.values(stored.metrics.counters).every(finite) &&
      Object.values(stored.metrics.durations).every((value: any) => value && finite(value.count) && value.count > 0 &&
        finite(value.totalMs) && finite(value.maxMs) && Array.isArray(value.buckets) &&
        value.buckets.length === 12 && value.buckets.every(finite)) &&
      Array.isArray(stored.metrics.slowest) && stored.metrics.slowest.length <= 30;
    if (stored?.schemaVersion === 2 && finite(stored.startedAt) && stored.startedAt > 0 &&
        stored.startedAt <= Date.now() && finite(stored.stoppedAt) && finite(stored.pages) && validMetrics &&
        stored.frames && Object.values(stored.frames).every(finite) && stored.peaks && stored.capabilities) {
      capture = stored;
      if (!capture.stoppedAt && Date.now() - capture.startedAt >= MAX_CAPTURE_MS) {
        capture.stoppedAt = capture.startedAt + MAX_CAPTURE_MS;
        persist();
      }
      if (!capture.stoppedAt) { capture.pages++; activate(); }
    }
    localStorage.removeItem('wunder_chat_perf');
  } catch { stopObservers(); running = false; capture = freshCapture(); }
  if (typeof window.addEventListener === 'function') {
    window.addEventListener('pagehide', () => { if (capture.startedAt) persist(); stopObservers(); });
    window.addEventListener('pageshow', () => {
      if (running && Date.now() - capture.startedAt >= MAX_CAPTURE_MS) chatPerf.stop();
      else if (running && !browser) activate();
    });
  }
  (window as Window & { wunderPerf?: typeof chatPerf }).wunderPerf = chatPerf;
}
