// W2-F: minimal single-worker pool for thread snapshot planning. It mirrors the
// existing `chatRenderWorker` pattern (one persistent worker, a bounded queue,
// terminate on abort, idle teardown) but is specialized for the snapshot shard
// planner. When `Worker` is unavailable (node regression tests) the caller
// receives `null` and falls back to planning on the main thread.
import type {
  ChatThreadSnapshot,
  ChatThreadSnapshotPlan,
  ChatThreadSnapshotPlanOptions
} from '@/realtime/chat/chatThreadState';

type WorkerFactory = () => Worker;

interface SnapshotJob {
  id: number;
  snapshot: ChatThreadSnapshot;
  options?: ChatThreadSnapshotPlanOptions;
  signal?: AbortSignal;
  resolve: (plan: ChatThreadSnapshotPlan | null) => void;
  abort: () => void;
}

const queue: SnapshotJob[] = [];
let active: SnapshotJob | null = null;
let worker: Worker | null = null;
let sequence = 0;
let idleTimer: ReturnType<typeof setTimeout> | undefined;
let workerFactory: WorkerFactory | null = null;

/**
 * Test seam: inject a worker factory (or null to reset). Production leaves this
 * null and uses a module worker built from the bundled worker entry.
 */
export const setChatThreadSnapshotWorkerFactory = (factory: WorkerFactory | null): void => {
  terminateChatThreadSnapshotWorker();
  workerFactory = factory;
};

const workerAvailable = (): boolean => workerFactory !== null || typeof Worker !== 'undefined';

const createWorker = (): Worker => workerFactory
  ? workerFactory()
  : new Worker(new URL('./chatThreadSnapshot.worker.ts', import.meta.url), { type: 'module' });

const destroyWorker = (): void => {
  if (idleTimer !== undefined) {
    clearTimeout(idleTimer);
    idleTimer = undefined;
  }
  worker?.terminate();
  worker = null;
};

const settle = (job: SnapshotJob, plan: ChatThreadSnapshotPlan | null): void => {
  job.signal?.removeEventListener('abort', job.abort);
  job.resolve(plan);
};

const finish = (plan: ChatThreadSnapshotPlan | null): void => {
  const job = active;
  active = null;
  if (job) settle(job, plan);
  pump();
};

const pump = (): void => {
  if (active) return;
  const next = queue.shift();
  if (!next) {
    if (idleTimer !== undefined) clearTimeout(idleTimer);
    idleTimer = setTimeout(() => destroyWorker(), 30000);
    return;
  }
  active = next;
  if (idleTimer !== undefined) {
    clearTimeout(idleTimer);
    idleTimer = undefined;
  }
  try {
    if (!worker) {
      worker = createWorker();
      worker.onerror = () => { destroyWorker(); finish(null); };
      worker.onmessage = (event: MessageEvent) => {
        const job = active;
        const data = event?.data as
          | { id?: number; plan?: ChatThreadSnapshotPlan; error?: boolean }
          | undefined;
        if (!job || !data || data.id !== job.id) return;
        finish(data.error ? null : (data.plan ?? null));
      };
    }
    worker.postMessage({ id: next.id, snapshot: next.snapshot, options: next.options });
  } catch {
    destroyWorker();
    finish(null);
  }
};

/**
 * Plan a snapshot in the worker. Resolves `null` when no worker is available,
 * when the signal is already aborted, or when the worker fails: callers then
 * plan synchronously on the main thread.
 */
export const planChatThreadSnapshotInWorker = (
  snapshot: ChatThreadSnapshot,
  options?: ChatThreadSnapshotPlanOptions,
  signal?: AbortSignal
): Promise<ChatThreadSnapshotPlan | null> => new Promise((resolve) => {
  if (!workerAvailable() || signal?.aborted) {
    resolve(null);
    return;
  }
  const job: SnapshotJob = {
    id: ++sequence, snapshot, options, signal, resolve,
    abort: () => {
      if (active === job) {
        // Drop the whole worker: a superseded plan must not block the next one.
        destroyWorker();
        finish(null);
      } else {
        const index = queue.indexOf(job);
        if (index >= 0) queue.splice(index, 1);
        settle(job, null);
      }
    }
  };
  signal?.addEventListener('abort', job.abort, { once: true });
  queue.push(job);
  pump();
});

export interface ChatThreadSnapshotWorkerStats {
  available: boolean;
  workerAlive: boolean;
  queued: number;
  active: boolean;
}

export const getChatThreadSnapshotWorkerStats = (): ChatThreadSnapshotWorkerStats => ({
  available: workerAvailable(),
  workerAlive: worker !== null,
  queued: queue.length,
  active: active !== null
});

/**
 * Tear the pool down and settle every pending request with `null`: no queue
 * entry, no active job and no live worker may survive the call.
 */
export const terminateChatThreadSnapshotWorker = (): void => {
  destroyWorker();
  const pending = queue.splice(0, queue.length);
  const running = active;
  active = null;
  if (running) settle(running, null);
  for (const job of pending) settle(job, null);
};
