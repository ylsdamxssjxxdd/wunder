import { getMarkdownRenderLabels } from '@/utils/markdown';

type RenderResult = { parsed?: unknown; html?: string; text?: string; truncated?: boolean };
type Job = {
  id: number;
  source: string;
  kind: 'markdown' | 'detail';
  resolvePath?: (path: string) => string;
  resolve: (result: RenderResult | null) => void;
  signal?: AbortSignal;
  abort: () => void;
};

const queue: Job[] = [];
let active: Job | null = null;
let worker: Worker | null = null;
let idleTimer: ReturnType<typeof setTimeout> | undefined;
let timeout: ReturnType<typeof setTimeout> | undefined;
let sequence = 0;
const MAX_QUEUED_BYTES = 16 * 1024 * 1024;

const destroyWorker = () => {
  worker?.terminate();
  worker = null;
  clearTimeout(timeout);
  clearTimeout(idleTimer);
};

const settle = (job: Job, result: RenderResult | null) => {
  job.signal?.removeEventListener('abort', job.abort);
  job.resolve(result);
};

const finish = (result: RenderResult | null) => {
  clearTimeout(timeout);
  const job = active;
  active = null;
  if (job) settle(job, result);
  pump();
};

const pump = () => {
  if (active) return;
  const next = queue.shift();
  if (!next) {
    clearTimeout(idleTimer);
    idleTimer = setTimeout(destroyWorker, 30000);
    return;
  }
  active = next;
  clearTimeout(idleTimer);
  try {
    if (!worker) {
      worker = new Worker(new URL('../../workers/chatRender.worker.ts', import.meta.url), { type: 'module' });
      worker.onerror = () => { destroyWorker(); finish(null); };
      worker.onmessage = ({ data }) => {
        const job = active;
        if (!job || data.id !== job.id) return;
        if (Array.isArray(data.paths)) {
          try {
            const paths = Object.fromEntries(data.paths.map((path: string) => [path, job.resolvePath?.(path) || '']));
            worker?.postMessage({ id: job.id, kind: job.kind, source: job.source,
              labels: getMarkdownRenderLabels(), paths });
          } catch { finish(null); }
          return;
        }
        finish(data.error ? null : data);
      };
    }
    timeout = setTimeout(() => { destroyWorker(); finish(null); }, 15000);
    worker.postMessage({ id: next.id, kind: next.kind, source: next.source,
      labels: getMarkdownRenderLabels(), paths: next.resolvePath ? undefined : {} });
  } catch { destroyWorker(); finish(null); }
};

export const renderChatInWorker = (
  kind: Job['kind'], source: string, signal: AbortSignal, resolvePath?: Job['resolvePath']
): Promise<RenderResult | null> => new Promise(resolve => {
  if (signal.aborted || typeof Worker === 'undefined' || queue.length >= 48 ||
      queue.reduce((bytes, job) => bytes + job.source.length * 2, source.length * 2) > MAX_QUEUED_BYTES) {
    resolve(null);
    return;
  }
  const job: Job = { id: ++sequence, kind, source, signal, resolvePath, resolve, abort: () => {
    if (active === job) {
      // A removed row must not leave a large parse blocking the next visible row.
      destroyWorker();
      finish(null);
    } else {
      const index = queue.indexOf(job);
      if (index >= 0) queue.splice(index, 1);
      settle(job, null);
    }
  } };
  signal.addEventListener('abort', job.abort, { once: true });
  queue.push(job);
  pump();
});
