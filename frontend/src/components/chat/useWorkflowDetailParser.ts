import { onBeforeUnmount, shallowRef } from 'vue';
import { renderChatInWorker } from './chatRenderWorker';

export const WORKFLOW_DETAIL_WORKER_THRESHOLD = 24000;

export const useWorkflowDetailParser = () => {
  const revision = shallowRef(0);
  const cache = new Map<string, { parsed: unknown; text: string }>();
  const pending = new Map<string, AbortController>();
  let disposed = false;
  let bytes = 0;
  const prepare = (sources: string[]) => {
    for (const source of sources) {
      if (source.length < WORKFLOW_DETAIL_WORKER_THRESHOLD || cache.has(source) || pending.has(source)) continue;
      if (pending.size >= 9) continue;
      const controller = new AbortController();
      pending.set(source, controller);
      void renderChatInWorker('detail', source, controller.signal).then(result => {
        pending.delete(source);
        if (disposed || controller.signal.aborted) return;
        const entry = { parsed: result?.parsed ?? null, text: result?.text ?? source.slice(0, 24000) };
        cache.set(source, entry);
        bytes += source.length * 4 + entry.text.length * 2;
        while (cache.size > 12 || (bytes > 8 * 1024 * 1024 && cache.size > 1)) {
          const oldest = cache.keys().next().value!;
          bytes -= oldest.length * 4 + cache.get(oldest)!.text.length * 2;
          cache.delete(oldest);
        }
        revision.value++;
      });
    }
  };
  onBeforeUnmount(() => {
    disposed = true;
    pending.forEach(controller => controller.abort());
    pending.clear();
    cache.clear();
  });
  return {
    revision, prepare,
    read: (source: string) => { void revision.value; return cache.get(source)?.parsed ?? null; },
    format: (source: string) => { void revision.value; return cache.get(source)?.text ?? source.slice(0, 24000); }
  };
};
