import { onBeforeUnmount, ref, watch, type Ref } from 'vue';

import { formatDurationSeconds, resolveAssistantTurnSeconds } from '@/utils/messageStats';

/**
 * 工作过程条右侧的本轮耗时。
 *
 * 运行时只会在统计帧到达时给出 interaction_duration_s，一帧可能要等好几十秒
 * （比如一次长工具调用），直接渲染就是「数字停住、然后跳一大截」。这里以最新
 * 统计值为锚点，用本地时钟逐秒补齐中间的时间；轮次结束后立刻交回权威统计值。
 *
 * 两条硬约束，都是踩过的坑：
 * 1. 校准只认「比当前显示更靠前」的统计值。相同值反复到达时若重设锚点，秒表
 *    每 250ms 就被拉回原点，数字就永远停在统计值上——看起来只在有新条目时跳。
 * 2. 锚点按轮次存在模块级表里。列表行会随每次统计更新重建，组件一重挂载就把
 *    实例内的锚点清零，时间又从权威值重新开始，那等于白走。
 */
type Anchor = { seconds: number; at: number; shown: number };

const ANCHOR_LIMIT = 64;
const anchors = new Map<string, Anchor>();

const anchorKey = (message: Record<string, any> | null | undefined): string => {
  const turn = String(
    message?.root_turn_id || message?.turn_id || message?.item_id || message?.id || ''
  );
  if (turn) return `${turn}:${String(message?.role || '')}`;
  return String(message?.role || '');
};

const readAnchor = (key: string): Anchor | undefined => anchors.get(key);

const writeAnchor = (key: string, anchor: Anchor) => {
  anchors.delete(key);
  anchors.set(key, anchor);
  while (anchors.size > ANCHOR_LIMIT) {
    const oldest = anchors.keys().next();
    if (oldest.done) break;
    anchors.delete(oldest.value);
  }
};

export const useTurnDurationLabel = (
  message: Ref<Record<string, any> | null | undefined>,
  running: Ref<boolean>
) => {
  const label = ref('');
  let timer: number | null = null;
  let anchorSeconds: number | null = null;
  let anchorAt = 0;
  let shown = 0;

  const stop = () => {
    if (timer !== null) {
      window.clearInterval(timer);
      timer = null;
    }
  };

  const settle = () => {
    const seconds = resolveAssistantTurnSeconds(message.value);
    label.value = seconds === null ? '' : formatDurationSeconds(seconds);
    anchors.delete(anchorKey(message.value));
  };

  const advance = () => {
    const authoritative = resolveAssistantTurnSeconds(message.value);
    const key = anchorKey(message.value);
    // 只有统计值真的跑到前面，才重新锚定；相同值（长工具调用期间反复重发）
    // 不能把锚点往后挪，否则秒表永远走不到下一格。
    const expected =
      anchorSeconds === null ? 0 : anchorSeconds + (Date.now() - anchorAt) / 1000;
    if (authoritative !== null && (anchorSeconds === null || authoritative > expected)) {
      anchorSeconds = authoritative;
      anchorAt = Date.now();
    }
    if (anchorSeconds === null) return;
    // 统计值的取整误差不应该让秒表往回跳：同一轮里只许前进。
    shown = Math.max(shown, Math.round(anchorSeconds + (Date.now() - anchorAt) / 1000));
    const next = formatDurationSeconds(shown);
    if (next !== label.value) label.value = next;
    writeAnchor(key, { seconds: anchorSeconds, at: anchorAt, shown });
  };

  // 轮次是否还在跑，不能只看内容流标记：一次长工具调用期间流式标记可能同时
  // 落成 false（没有新 token、没有新条目），但过程条还在、秒表还得走。那时若
  // 停下，用户看到的就是「跟着工具条目一起跳」。
  const RUNNING_STATUSES = new Set(['running', 'queued', 'waiting', 'compacting']);
  const isLive = () =>
    running.value ||
    RUNNING_STATUSES.has(
      String(message.value?.status ?? message.value?.stats_status ?? '')
        .trim()
        .toLowerCase()
    );

  watch(
    () => [isLive(), resolveAssistantTurnSeconds(message.value)] as const,
    ([live]) => {
      if (!live) {
        stop();
        settle();
        return;
      }
      // 重挂载：接着上一段秒表走，而不是从权威统计值重新开始。
      if (anchorSeconds === null) {
        const restored = readAnchor(anchorKey(message.value));
        if (restored) {
          anchorSeconds = restored.seconds;
          anchorAt = restored.at;
          shown = restored.shown;
        }
      }
      // 首帧统计还没到：从「开始跑」这一刻先本地计数。否则过程条右侧在第一个
      // 统计帧到达前是空的，用户看不出这一轮已经在计时。
      if (anchorSeconds === null && resolveAssistantTurnSeconds(message.value) === null) {
        anchorSeconds = 0;
        anchorAt = Date.now();
      }
      advance();
      if (timer === null) timer = window.setInterval(advance, 250);
    },
    { immediate: true }
  );

  onBeforeUnmount(stop);
  return label;
};
