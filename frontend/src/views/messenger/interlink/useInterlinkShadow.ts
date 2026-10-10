// AI生成
/**
 * 节点影子投影的取数（方案 §6.1 白名单 + §6.2 revision 语义）。
 *
 * 只在「面板挂载 / 用户点刷新 / 显式 `shadow.refresh`」时拉：本地繁忙时 revision
 * 每 2s 就会变，跟着轮询会把用户的展开状态反复冲掉，也让服务端读放大。
 * 时间戳水印负责告诉用户这份投影有多旧（§6.3）。
 */

import { onScopeDispose, getCurrentScope, ref } from 'vue';

import { fetchInterlinkShadow } from '@/api/interlink';
import type { InterlinkShadow } from '@/api/interlink';
import { resolveApiError } from '@/utils/apiError';
import { INTERLINK_REFRESH_DEBOUNCE_MS } from './interlinkNodeModel';

export const useInterlinkShadow = () => {
  const shadow = ref<InterlinkShadow | null>(null);
  const loading = ref(false);
  const error = ref('');
  const loadedAt = ref(0);

  let inflight: AbortController | null = null;
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  let serial = 0;

  const load = async (deviceId: string): Promise<void> => {
    const target = String(deviceId || '').trim();
    if (!target) {
      shadow.value = null;
      error.value = '';
      return;
    }
    if (inflight) inflight.abort();
    const controller = new AbortController();
    inflight = controller;
    const currentSerial = ++serial;
    loading.value = true;
    try {
      const next = await fetchInterlinkShadow(target, { signal: controller.signal });
      if (currentSerial !== serial) return;
      shadow.value = next;
      error.value = '';
      loadedAt.value = Date.now();
    } catch (caught) {
      if (currentSerial !== serial || controller.signal.aborted) return;
      error.value = resolveApiError(caught, '').message || '';
    } finally {
      if (currentSerial === serial) loading.value = false;
      if (inflight === controller) inflight = null;
    }
  };

  /** 去抖刷新：连点按钮或「刷新 + 命令回填」同时发生时只发一次请求。 */
  const refresh = (deviceId: string, delayMs = INTERLINK_REFRESH_DEBOUNCE_MS): void => {
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      debounceTimer = null;
      void load(deviceId);
    }, Math.max(0, Number(delayMs) || 0));
  };

  const reset = (): void => {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
    if (inflight) {
      inflight.abort();
      inflight = null;
    }
    serial += 1;
    shadow.value = null;
    error.value = '';
    loading.value = false;
    loadedAt.value = 0;
  };

  const teardown = (): void => {
    reset();
  };

  if (getCurrentScope()) onScopeDispose(teardown);

  return { shadow, loading, error, loadedAt, load, refresh, reset, teardown };
};
