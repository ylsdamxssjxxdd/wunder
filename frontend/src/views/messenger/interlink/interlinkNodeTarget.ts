// AI生成
/**
 * 工作区「节点切换器」的选中态（方案 §6.3：URL 形态 `/workspace?node=device:<id>`）。
 *
 * 唯一事实源是路由 query：蜂巢的工作区面具有两个宿主（侧栏云端目录区与通用工作区
 * 面板），用路由而不是本地状态，天然一致、可分享、支持前进后退，也不需要任何
 * watch / 全局 store（避免本仓库反复出现的多入口状态漂移）。
 */

import { computed } from 'vue';
import { useRoute, useRouter } from 'vue-router';

import { INTERLINK_CLOUD_NODE_ID, interlinkDeviceIdOf, interlinkDeviceTarget } from '@/api/interlink';

export const INTERLINK_NODE_QUERY_KEY = 'node';

export type InterlinkNodeTarget = string;

export const CLOUD_NODE_TARGET = INTERLINK_CLOUD_NODE_ID;

/** 允许的值：`cloud`（默认）、`device:<id>`；其它一律回落云端，绝不注入未知目标。 */
export const normalizeNodeTarget = (value: unknown): InterlinkNodeTarget => {
  const raw = Array.isArray(value) ? value[0] : value;
  // 设备 id 大小写敏感，只做 trim，不整体小写。
  const text = String(raw ?? '').trim();
  if (!text || text.toLowerCase() === CLOUD_NODE_TARGET) return CLOUD_NODE_TARGET;
  if (text.toLowerCase().startsWith('device:')) {
    const deviceId = text.slice('device:'.length).trim();
    return deviceId ? interlinkDeviceTarget(deviceId) : CLOUD_NODE_TARGET;
  }
  // 允许直接写设备 id（等价 device:<id>），分享链接时更短。
  return interlinkDeviceTarget(text);
};

export const useInterlinkNodeTarget = () => {
  const route = useRoute();
  const router = useRouter();

  const target = computed<InterlinkNodeTarget>(() =>
    normalizeNodeTarget(route.query[INTERLINK_NODE_QUERY_KEY])
  );
  const isRemote = computed(() => target.value !== CLOUD_NODE_TARGET);
  const deviceId = computed(() => interlinkDeviceIdOf(target.value));

  const writeTarget = (next: InterlinkNodeTarget): void => {
    const normalized = normalizeNodeTarget(next);
    const current = normalizeNodeTarget(route.query[INTERLINK_NODE_QUERY_KEY]);
    if (normalized === current) return;
    const query = { ...route.query } as Record<string, unknown>;
    if (normalized === CLOUD_NODE_TARGET) {
      delete query[INTERLINK_NODE_QUERY_KEY];
    } else {
      query[INTERLINK_NODE_QUERY_KEY] = normalized;
    }
    void router
      .replace({ path: route.path, query, hash: route.hash })
      .catch(() => undefined);
  };

  return { target, deviceId, isRemote, writeTarget, backToCloud: () => writeTarget(CLOUD_NODE_TARGET) };
};
