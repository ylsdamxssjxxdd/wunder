import { onBeforeUnmount, ref, watch, type Ref } from 'vue';

import { formatDurationSeconds, resolveAssistantTurnSeconds } from '@/utils/messageStats';

/**
 * 工作过程条右侧的本轮耗时。
 *
 * 运行时只会在统计帧到达时给出 interaction_duration_s，一帧可能要等好几十秒
 * （比如一次长工具调用），直接渲染就是「数字停住、然后跳一大截」。这里以最新
 * 统计值为锚点，用本地时钟逐秒补齐中间的时间；轮次结束后立刻交回权威统计值。
 */
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
  };

  const advance = () => {
    const authoritative = resolveAssistantTurnSeconds(message.value);
    if (authoritative !== null && (anchorSeconds === null || authoritative >= anchorSeconds)) {
      anchorSeconds = authoritative;
      anchorAt = Date.now();
    }
    if (anchorSeconds === null) return;
    // 统计值的取整误差不应该让秒表往回跳：同一轮里只许前进。
    shown = Math.max(shown, Math.round(anchorSeconds + (Date.now() - anchorAt) / 1000));
    const next = formatDurationSeconds(shown);
    if (next !== label.value) label.value = next;
  };

  watch(
    () => [running.value, resolveAssistantTurnSeconds(message.value)] as const,
    ([isRunning]) => {
      if (!isRunning) {
        stop();
        settle();
        return;
      }
      advance();
      if (timer === null) timer = window.setInterval(advance, 250);
    },
    { immediate: true }
  );

  onBeforeUnmount(stop);
  return label;
};
