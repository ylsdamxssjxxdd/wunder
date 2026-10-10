// AI生成
/**
 * 「我的设备」节点目录的取数与轮询（方案 §5.3 / §5.1）。
 *
 * 性能与生命周期约束（AGENTS.md「有界、限量」+ 本仓库已知的 watch/定时器泄漏坑）：
 * - 轮询周期固定慢节奏（≥15s），且只在「面板可见」时跑；
 * - 面板 KeepAlive 停用、页面隐藏、组件卸载三种路径都会清表；
 * - 手动刷新走去抖，同一时刻只有一个请求在飞（AbortController + 序号防回填）；
 * - 渲染条数有硬上限，加载更多只累加已渲染数量，绝不一次拉全量。
 */

import { computed, getCurrentScope, onScopeDispose, ref, watch } from 'vue';
import type { Ref } from 'vue';

import { fetchInterlinkNodes } from '@/api/interlink';
import type { InterlinkNode } from '@/api/interlink';
import { resolveApiError } from '@/utils/apiError';
import {
  INTERLINK_NODE_MAX_RENDERED,
  INTERLINK_NODE_PAGE_SIZE,
  INTERLINK_POLL_INTERVAL_MS,
  INTERLINK_REFRESH_DEBOUNCE_MS,
  isDeviceNode,
  isNodeReachable
} from './interlinkNodeModel';

type UseInterlinkNodesOptions = {
  /** 面板是否处于可见状态（KeepAlive 停用 / 页面隐藏时传 false）。 */
  active?: Ref<boolean>;
  pollIntervalMs?: number;
  pageSize?: number;
};

export const useInterlinkNodes = (options: UseInterlinkNodesOptions = {}) => {
  const pageSize = Math.max(
    1,
    Math.min(INTERLINK_NODE_MAX_RENDERED, Number(options.pageSize) || INTERLINK_NODE_PAGE_SIZE)
  );
  const pollIntervalMs = Math.max(15_000, Number(options.pollIntervalMs) || INTERLINK_POLL_INTERVAL_MS);

  const nodes = ref<InterlinkNode[]>([]);
  const aggregateStatus = ref('offline');
  const onlineCount = ref(0);
  const total = ref(0);
  const loading = ref(false);
  const error = ref('');
  const lastLoadedAt = ref(0);
  const renderedLimit = ref(pageSize);

  let pollTimer: ReturnType<typeof setInterval> | null = null;
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  let inflight: AbortController | null = null;
  let requestSerial = 0;
  let polling = false;

  const deviceNodes = computed(() => nodes.value.filter(isDeviceNode));
  const onlineDeviceCount = computed(() => deviceNodes.value.filter(isNodeReachable).length);

  // 上一次请求正好填满已渲染额度，说明服务端还有下一页。
  const hasMore = computed(
    () => renderedLimit.value < INTERLINK_NODE_MAX_RENDERED && nodes.value.length >= renderedLimit.value
  );

  const isPageVisible = (): boolean =>
    typeof document === 'undefined' || document.visibilityState !== 'hidden';

  const load = async (reason: 'initial' | 'poll' | 'manual' = 'manual'): Promise<void> => {
    // 只允许一个在飞请求；轮询撞上手动刷新时直接放弃这次轮询。
    if (inflight) {
      if (reason === 'poll') return;
      inflight.abort();
    }
    const controller = new AbortController();
    inflight = controller;
    const serial = ++requestSerial;
    if (reason !== 'poll') loading.value = true;
    try {
      const page = await fetchInterlinkNodes(
        { limit: renderedLimit.value, offset: 0 },
        { signal: controller.signal }
      );
      if (serial !== requestSerial) return;
      nodes.value = page.nodes.slice(0, INTERLINK_NODE_MAX_RENDERED);
      aggregateStatus.value = page.aggregate_status;
      onlineCount.value = page.online_count;
      total.value = page.total;
      error.value = '';
      lastLoadedAt.value = Date.now();
    } catch (caught) {
      if (serial !== requestSerial || controller.signal.aborted) return;
      error.value = resolveApiError(caught, '').message || '';
    } finally {
      if (serial === requestSerial) loading.value = false;
      if (inflight === controller) inflight = null;
    }
  };

  /** 去抖后的手动刷新（切分类、点刷新、开关 kill switch 之后）。 */
  const refresh = (): void => {
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      debounceTimer = null;
      void load('manual');
    }, INTERLINK_REFRESH_DEBOUNCE_MS);
  };

  const loadMore = (): void => {
    if (renderedLimit.value >= INTERLINK_NODE_MAX_RENDERED) return;
    renderedLimit.value = Math.min(
      INTERLINK_NODE_MAX_RENDERED,
      renderedLimit.value + pageSize
    );
    void load('manual');
  };

  const startPolling = (): void => {
    if (polling) return;
    polling = true;
    if (!nodes.value.length) void load('initial');
    pollTimer = setInterval(() => {
      void load('poll');
    }, pollIntervalMs);
  };

  const stopPolling = (): void => {
    polling = false;
    if (pollTimer) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
  };

  const shouldBePolling = (): boolean => (options.active ? options.active.value !== false : true);

  const handleVisibilityChange = (): void => {
    if (!isPageVisible()) {
      stopPolling();
      return;
    }
    if (shouldBePolling()) {
      // 回到前台先补水一次，避免用户盯着过期状态点。
      void load('poll');
      startPolling();
    }
  };

  const teardown = (): void => {
    stopPolling();
    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
    if (inflight) {
      inflight.abort();
      inflight = null;
    }
    if (typeof document !== 'undefined') {
      document.removeEventListener('visibilitychange', handleVisibilityChange);
    }
  };

  watch(
    () => (options.active ? options.active.value : true),
    (active) => {
      if (active === false) {
        stopPolling();
        return;
      }
      if (!isPageVisible()) return;
      startPolling();
    }
  );

  if (typeof document !== 'undefined') {
    document.addEventListener('visibilitychange', handleVisibilityChange);
  }

  if (getCurrentScope()) onScopeDispose(teardown);

  if (shouldBePolling() && isPageVisible()) {
    startPolling();
  }

  return {
    nodes,
    deviceNodes,
    onlineDeviceCount,
    aggregateStatus,
    onlineCount,
    total,
    loading,
    error,
    lastLoadedAt,
    hasMore,
    load,
    refresh,
    loadMore,
    startPolling,
    stopPolling,
    teardown
  };
};
