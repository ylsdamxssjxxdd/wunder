type Sink = {
  duration: (name: string, ms: number) => void;
  frame: (ms: number) => void;
  dom: (values: { elements: number; messages: number; tools: number; heapBytes: number | null }) => void;
};

export const observeChatBrowserPerformance = (sink: Sink) => {
  let previous = 0;
  let frame = 0;
  let disposed = false;
  const startedAt = performance.now();
  let visibleSince = document.hidden ? Number.POSITIVE_INFINITY : startedAt;
  const tick = (now: number) => {
    if (disposed || document.hidden) return;
    if (previous) sink.frame(now - previous);
    previous = now;
    frame = requestAnimationFrame(tick);
  };
  const visibility = () => {
    cancelAnimationFrame(frame);
    previous = 0;
    visibleSince = document.hidden ? Number.POSITIVE_INFINITY : performance.now();
    if (!document.hidden) frame = requestAnimationFrame(tick);
  };
  const observers: PerformanceObserver[] = [];
  const supported = typeof PerformanceObserver !== 'undefined' ? PerformanceObserver.supportedEntryTypes || [] : [];
  const capabilities = { longTask: false, inputEvent: false, heap: 'memory' in performance };
  const observe = (type: string, metric: string) => {
    if (!supported.includes(type)) return false;
    try {
      const observer = new PerformanceObserver(list => {
        if (disposed || document.hidden) return;
        for (const entry of list.getEntries()) {
          if (entry.startTime >= Math.max(startedAt, visibleSince)) sink.duration(metric, entry.duration);
        }
      });
      observer.observe({ type, buffered: false, ...(type === 'event' ? { durationThreshold: 40 } : {}) });
      observers.push(observer);
      return true;
    } catch { return false; }
  };
  capabilities.longTask = observe('longtask', 'browser_long_task');
  capabilities.inputEvent = observe('event', 'browser_input_event');
  const sampleDom = () => {
    if (disposed || document.hidden) return;
    const memory = (performance as Performance & { memory?: { usedJSHeapSize: number } }).memory;
    sink.dom({ elements: document.getElementsByTagName('*').length,
      messages: document.getElementsByClassName('messenger-message').length,
      tools: document.getElementsByClassName('tool-workflow-entry').length,
      heapBytes: memory?.usedJSHeapSize ?? null });
  };
  visibility();
  sampleDom();
  const timer = window.setInterval(sampleDom, 3000);
  document.addEventListener('visibilitychange', visibility);
  return {
    capabilities,
    dispose: () => {
      disposed = true;
      cancelAnimationFrame(frame);
      clearInterval(timer);
      observers.forEach(observer => observer.disconnect());
      document.removeEventListener('visibilitychange', visibility);
    }
  };
};
