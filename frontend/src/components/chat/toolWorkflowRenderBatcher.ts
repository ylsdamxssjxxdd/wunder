export type ToolWorkflowRenderBatcher = {
  request: (immediate?: boolean) => void;
  dispose: () => void;
};

type ToolWorkflowRenderBatcherOptions = {
  intervalMs?: number;
  now?: () => number;
};

// Tool streams can emit many small deltas per second. Coalesce their display
// updates so keyboard input and compositor work keep frame priority.
export const createToolWorkflowRenderBatcher = (
  flush: () => void,
  options: ToolWorkflowRenderBatcherOptions = {}
): ToolWorkflowRenderBatcher => {
  const intervalMs = Math.max(16, Math.trunc(options.intervalMs ?? 96));
  const now = options.now || (() => Date.now());
  let lastFlushedAt = Number.NEGATIVE_INFINITY;
  let timer: number | null = null;

  const run = () => {
    timer = null;
    lastFlushedAt = now();
    flush();
  };

  return {
    request(immediate = false) {
      if (typeof window === 'undefined') {
        if (timer !== null) {
          clearTimeout(timer);
          timer = null;
        }
        run();
        return;
      }
      if (immediate) {
        if (timer !== null) {
          window.clearTimeout(timer);
          timer = null;
        }
        run();
        return;
      }
      if (timer !== null) return;
      const delay = Math.max(0, intervalMs - (now() - lastFlushedAt));
      timer = window.setTimeout(run, delay);
    },
    dispose() {
      if (timer === null) return;
      clearTimeout(timer);
      timer = null;
    }
  };
};
