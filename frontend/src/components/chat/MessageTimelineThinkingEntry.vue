<template>
  <div class="tl-thinking" :class="{ 'is-open': open, 'is-streaming': entry.streaming }">
    <button
      class="tl-thinking-head"
      type="button"
      :aria-expanded="open"
      @click="emit('toggle')"
    >
      <span class="tl-gutter" aria-hidden="true">
        <span class="tl-gutter-line"></span>
        <span class="tl-thinking-status" :class="{ 'is-streaming': entry.streaming }">
          <i v-if="entry.streaming" class="fa-solid fa-circle-notch fa-spin"></i>
          <i v-else class="fa-solid fa-check"></i>
        </span>
      </span>
      <i class="fa-solid fa-brain tl-thinking-icon" aria-hidden="true"></i>
      <span class="tl-thinking-label">
        {{ entry.streaming ? t('chat.timeline.thinkingRunning') : t('chat.timeline.thinkingDone') }}
      </span>
      <span v-if="entry.summary" ref="summaryRef" class="tl-thinking-summary" :title="entry.summary">{{ entry.summary }}</span>
      <!-- 与工具条目同一套右侧列：模型轮次 + 该轮请求的上下文占用 + 思考耗时。 -->
      <span v-if="roundLabel || tokenMetric || durationMetric" class="tl-thinking-metrics">
        <span>{{ roundLabel }}</span>
        <span>{{ tokenMetric }}</span>
        <span>{{ durationMetric }}</span>
      </span>
    </button>

    <div v-if="open" class="tl-thinking-body">
      <pre ref="textRef" class="tl-thinking-text" @scroll="handleTextScroll">{{ entry.text || t('chat.timeline.thinkingEmpty') }}</pre>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import { useI18n } from '@/i18n';
import type { TimelineReasoningEntry } from './toolTimelineModel';
import { formatDurationLabel, formatTokenLabel } from './toolWorkflowCollapsedMetadata';

const props = defineProps<{
  entry: TimelineReasoningEntry;
  open?: boolean;
}>();

const tokenMetric = computed(() => formatTokenLabel(props.entry.metrics?.tokens ?? null));
const durationMetric = computed(() => formatDurationLabel(props.entry.metrics?.durationMs ?? null));
/** 模型轮次与工具条目同形：R1、R2…；旧记录没有轮次时留空。 */
const roundLabel = computed(() => {
  const round = props.entry.modelRound ?? null;
  return round !== null && round > 0 ? `R${round}` : '';
});

const textRef = ref<HTMLElement | null>(null);
const summaryRef = ref<HTMLElement | null>(null);
const followTail = ref(true);

const handleTextScroll = (): void => {
  const element = textRef.value;
  if (!element) return;
  followTail.value = element.scrollHeight - element.clientHeight - element.scrollTop <= 24;
};

const scrollToLatest = async (): Promise<void> => {
  await nextTick();
  const element = textRef.value;
  if (!element || !props.open || !followTail.value) return;
  element.scrollTop = element.scrollHeight;
};

const scrollSummaryToLatest = (): void => {
  const summary = summaryRef.value;
  if (summary && props.entry.streaming) summary.scrollLeft = summary.scrollWidth;
};

onMounted(() => {
  void scrollToLatest();
  scrollSummaryToLatest();
});

watch(
  () => [props.open, props.entry.text, props.entry.streaming] as const,
  ([open], [wasOpen]) => {
    if (open && !wasOpen) followTail.value = true;
    if (open) void scrollToLatest();
    scrollSummaryToLatest();
  },
  { flush: 'post' }
);

const emit = defineEmits<{ (event: 'toggle'): void }>();

const { t } = useI18n();
</script>

<style scoped>
/*
 * §7.3 B 思考条目：与工具条目共用同一套 `FoldEntry` 行度量
 * （行高 26px、间距 8px、沟槽 20px、状态点 12px，不显示右侧展开箭头）。
 */
.tl-thinking {
  min-width: 0;
}

.tl-thinking-head {
  display: flex;
  align-items: center;
  gap: 8px;
  box-sizing: border-box;
  width: 100%;
  height: 26px;
  padding: 0 24px;
  margin: 0;
  border: 0;
  background: var(--mz-panel, #fbfaf8);
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.tl-thinking-head:hover {
  background: var(--mz-timeline-hover, #f6f5f3);
}

.tl-thinking-head:focus-visible {
  outline: 2px solid rgba(var(--ui-accent-rgb), 0.4);
  outline-offset: -2px;
}

.tl-gutter {
  position: relative;
  flex: 0 0 auto;
  align-self: stretch;
  width: 20px;
  pointer-events: none;
}

.tl-gutter-line {
  position: absolute;
  top: 0;
  bottom: 0;
  left: 9.5px;
  width: 1px;
  background: var(--mz-timeline-line, #e2dfda);
}

.tl-thinking-status {
  position: absolute;
  z-index: 1;
  top: 7px;
  left: 3.5px;
  width: 12px;
  height: 12px;
  box-sizing: border-box;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--mz-timeline-entry-soft, #9ba0a8);
  border-radius: 50%;
  font-size: 7px;
  line-height: 1;
  color: var(--mz-text-muted, #8a8f99);
  background: var(--mz-panel, #fbfaf8);
}

.tl-thinking-status.is-streaming {
  border: 0;
  font-size: 12px;
  color: var(--mz-text-muted, #8a8f99);
}

.tl-thinking-icon {
  flex: 0 0 auto;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 13px;
  line-height: 18px;
}

.tl-thinking-label {
  flex: 0 0 auto;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 13px;
  /* 与工具条目同口径：标签和右侧摘要共用 18px 行高，混排时落在同一水平线。 */
  line-height: 18px;
}

.tl-thinking-summary {
  flex: 1 1 auto;
  min-width: 0;
  color: var(--mz-timeline-entry-soft, #9ba0a8);
  font-size: 12px;
  line-height: 18px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.is-streaming .tl-thinking-summary {
  text-overflow: clip;
}

/* 右侧三列与工具条目完全同宽同字：轮次 / token / 耗时跨条目从同一位置起排。 */
.tl-thinking-metrics {
  flex: 0 0 auto;
  display: inline-grid;
  grid-template-columns: 3ch 6ch 5ch;
  column-gap: 8px;
  justify-items: start;
  margin-left: auto;
  color: var(--mz-text-muted, #8a8f99);
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 11px;
  line-height: 18px;
  white-space: nowrap;
}

/* 展开内容左内边距保持 24px（沟槽 20px + 24px = 44px）。 */
.tl-thinking-body {
  margin: 4px 24px 8px 44px;
}

.tl-thinking-text {
  margin: 0;
  max-height: 240px;
  overflow: auto;
  padding: 8px 10px;
  border: 1px solid var(--mz-border, #e8e6e3);
  border-radius: 8px;
  background: var(--mz-panel, #fbfaf8);
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: inherit;
}
</style>
