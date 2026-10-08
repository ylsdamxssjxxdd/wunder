/**
 * Bounded upload queue for the sidebar file area (B2).
 *
 * Guarantees required by the plan (§6.6):
 * - at most `concurrency` (<= 2) requests in flight;
 * - the pending queue itself is bounded (`maxQueue`), extra files are rejected
 *   with a visible reason instead of growing without limit;
 * - progress events only mutate a plain map; the reactive projection is flushed
 *   at most once per animation frame, so a large file cannot trigger a render
 *   per chunk;
 * - every task can be cancelled (AbortController) and retried.
 */

import { reactive } from 'vue';

import {
  isWorkspaceRequestCancelled,
  resolveWorkspaceErrorMessage,
  uploadWorkspaceEntry
} from './workspaceFileApi';
import { normalizeWorkspaceRelativePath } from './workspaceFileModel';

export type WorkspaceUploadStatus = 'queued' | 'uploading' | 'done' | 'failed' | 'cancelled';

export type WorkspaceUploadTask = {
  id: string;
  name: string;
  targetPath: string;
  relativePath: string;
  size: number;
  loaded: number;
  status: WorkspaceUploadStatus;
  error: string;
  uploadedPaths: string[];
};

export type WorkspaceUploadQueueOptions = {
  concurrency?: number;
  maxQueue?: number;
  /** Called once per completed task, coalesced with the frame flush. */
  onUploaded?: (task: WorkspaceUploadTask) => void;
  /** Called when the queue drains (all tasks settled). */
  onDrained?: () => void;
};

const UPLOAD_MAX_CONCURRENCY = 2;
const UPLOAD_MAX_QUEUE = 24;

type InternalTask = WorkspaceUploadTask & {
  file: File;
  controller: AbortController | null;
};

let uploadTaskSeq = 0;

export const createWorkspaceUploadQueue = (options: WorkspaceUploadQueueOptions = {}) => {
  const concurrency = Math.max(1, Math.min(UPLOAD_MAX_CONCURRENCY, Number(options.concurrency) || 2));
  const maxQueue = Math.max(1, Number(options.maxQueue) || UPLOAD_MAX_QUEUE);

  const state = reactive({
    tasks: [] as WorkspaceUploadTask[]
  });

  const internal = new Map<string, InternalTask>();
  const pending: string[] = [];
  const progressBuffer = new Map<string, { loaded: number; total: number }>();
  const uploadedBuffer: WorkspaceUploadTask[] = [];
  let activeCount = 0;
  let frameHandle = 0;

  const flushFrame = () => {
    frameHandle = 0;
    if (progressBuffer.size) {
      progressBuffer.forEach((value, id) => {
        const task = internal.get(id);
        if (!task) return;
        task.loaded = value.total > 0 ? Math.min(value.loaded, value.total) : value.loaded;
        if (value.total > 0) task.size = value.total;
        const index = state.tasks.findIndex((item) => item.id === id);
        if (index >= 0) {
          state.tasks[index] = projectTask(task);
        }
      });
      progressBuffer.clear();
    }
    if (uploadedBuffer.length) {
      const flushed = uploadedBuffer.splice(0, uploadedBuffer.length);
      flushed.forEach((task) => options.onUploaded?.(task));
    }
  };

  const scheduleFlush = () => {
    if (frameHandle) return;
    if (typeof window === 'undefined' || typeof window.requestAnimationFrame !== 'function') {
      flushFrame();
      return;
    }
    frameHandle = window.requestAnimationFrame(flushFrame);
  };

  const projectTask = (task: InternalTask): WorkspaceUploadTask => ({
    id: task.id,
    name: task.name,
    targetPath: task.targetPath,
    relativePath: task.relativePath,
    size: task.size,
    loaded: task.loaded,
    status: task.status,
    error: task.error,
    uploadedPaths: task.uploadedPaths
  });

  const syncTask = (id: string) => {
    const task = internal.get(id);
    // A disposed queue clears its task map; late settle callbacks are ignored.
    if (!task) return;
    const index = state.tasks.findIndex((item) => item.id === id);
    const projected = projectTask(task);
    if (index >= 0) {
      state.tasks[index] = projected;
    } else {
      state.tasks.push(projected);
    }
  };

  const pump = () => {
    while (activeCount < concurrency && pending.length) {
      const id = pending.shift();
      if (!id) break;
      void runTask(id);
    }
  };

  /** Keep the reactive list identical in order to the internal task map. */
  const rebuildTaskList = () => {
    const next = Array.from(internal.values()).map(projectTask);
    state.tasks.splice(0, state.tasks.length, ...next);
  };

  const finishTask = (task: InternalTask) => {
    activeCount = Math.max(0, activeCount - 1);
    syncTask(task.id);
    if (task.status === 'done') {
      uploadedBuffer.push(projectTask(task));
      scheduleFlush();
    }
    if (!pending.length && activeCount === 0) {
      flushFrame();
      options.onDrained?.();
    }
  };

  const runTask = async (id: string) => {
    const task = internal.get(id);
    if (!task) return;
    task.status = 'uploading';
    task.error = '';
    task.controller = new AbortController();
    syncTask(id);
    activeCount += 1;
    try {
      const outcome = await uploadWorkspaceEntry(task.file, task.targetPath, {
        relativePath: task.relativePath,
        signal: task.controller.signal,
        onProgress: (loaded, total) => {
          const buffered = progressBuffer.get(id);
          progressBuffer.set(id, {
            loaded,
            total: total || buffered?.total || task.size
          });
          scheduleFlush();
        }
      });
      task.loaded = task.size;
      task.uploadedPaths = outcome.files;
      task.status = 'done';
    } catch (error) {
      if (isWorkspaceRequestCancelled(error)) {
        task.status = 'cancelled';
      } else {
        task.status = 'failed';
        task.error = resolveWorkspaceErrorMessage(error);
      }
    } finally {
      task.controller = null;
      finishTask(task);
      pump();
    }
  };

  const enqueue = (
    files: Array<{ file: File; relativePath?: string }>,
    targetPath: string
  ): { accepted: number; rejected: number } => {
    const normalizedTarget = normalizeWorkspaceRelativePath(targetPath);
    let accepted = 0;
    let rejected = 0;
    files.forEach((item) => {
      const file = item?.file;
      if (!file) return;
      if (internal.size >= maxQueue) {
        rejected += 1;
        return;
      }
      uploadTaskSeq += 1;
      const id = `ws-upload-${uploadTaskSeq}`;
      const task: InternalTask = {
        id,
        name: file.name,
        targetPath: normalizedTarget,
        relativePath: String(item.relativePath || '').trim(),
        size: Number(file.size) || 0,
        loaded: 0,
        status: 'queued',
        error: '',
        uploadedPaths: [],
        file,
        controller: null
      };
      internal.set(id, task);
      pending.push(id);
      accepted += 1;
    });
    if (accepted) {
      rebuildTaskList();
      pump();
    }
    return { accepted, rejected };
  };

  const cancel = (id: string) => {
    const task = internal.get(id);
    if (!task) return;
    const pendingIndex = pending.indexOf(id);
    if (pendingIndex >= 0) {
      pending.splice(pendingIndex, 1);
      task.status = 'cancelled';
      syncTask(id);
      return;
    }
    task.controller?.abort();
  };

  const retry = (id: string) => {
    const task = internal.get(id);
    if (!task || task.status === 'uploading' || task.status === 'queued') return;
    task.status = 'queued';
    task.loaded = 0;
    task.error = '';
    pending.push(id);
    syncTask(id);
    pump();
  };

  const cancelAll = () => {
    pending.splice(0, pending.length).forEach((id) => {
      const task = internal.get(id);
      if (task) {
        task.status = 'cancelled';
        syncTask(id);
      }
    });
    internal.forEach((task) => {
      if (task.status === 'uploading') task.controller?.abort();
    });
  };

  const clearSettled = () => {
    const removable = state.tasks.filter(
      (task) => task.status !== 'uploading' && task.status !== 'queued'
    );
    if (!removable.length) return;
    removable.forEach((task) => internal.delete(task.id));
    rebuildTaskList();
  };

  /** Keep failed tasks (so they can be retried) and drop the rest. */
  const clearSucceeded = () => {
    const removable = state.tasks.filter(
      (task) => task.status === 'done' || task.status === 'cancelled'
    );
    if (!removable.length) return;
    removable.forEach((task) => internal.delete(task.id));
    rebuildTaskList();
  };

  const dispose = () => {
    cancelAll();
    if (frameHandle && typeof window !== 'undefined') {
      window.cancelAnimationFrame(frameHandle);
      frameHandle = 0;
    }
    progressBuffer.clear();
    uploadedBuffer.length = 0;
    internal.clear();
    state.tasks.splice(0, state.tasks.length);
  };

  return {
    get tasks(): WorkspaceUploadTask[] {
      return state.tasks;
    },
    maxQueue,
    concurrency,
    enqueue,
    cancel,
    retry,
    cancelAll,
    clearSettled,
    clearSucceeded,
    dispose
  };
};

export type WorkspaceUploadQueue = ReturnType<typeof createWorkspaceUploadQueue>;
