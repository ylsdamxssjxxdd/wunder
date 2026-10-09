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
        <span class="tl-gutter-node" :class="{ 'is-loading': entry.streaming }"></span>
      </span>
      <span class="tl-thinking-status" :class="{ 'is-streaming': entry.streaming }" aria-hidden="true">
        <i v-if="entry.streaming" class="fa-solid fa-circle-notch fa-spin"></i>
        <i v-else class="fa-solid fa-check"></i>
      </span>
      <span class="tl-thinking-label">
        {{ entry.streaming ? t('chat.timeline.thinkingRunning') : t('chat.timeline.thinkingDone') }}
      </span>
      <span v-if="entry.summary" class="tl-thinking-summary" :title="entry.summary">{{ entry.summary }}</span>
      <i
        :class="['fa-solid', open ? 'fa-chevron-down' : 'fa-chevron-right', 'tl-thinking-toggle']"
        aria-hidden="true"
      ></i>
    </button>

    <div v-if="open" class="tl-thinking-body">
      <pre class="tl-thinking-text">{{ entry.text || t('chat.timeline.thinkingEmpty') }}</pre>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from '@/i18n';
import type { TimelineReasoningEntry } from './toolTimelineModel';

defineProps<{
  entry: TimelineReasoningEntry;
  open?: boolean;
}>();

const emit = defineEmits<{ (event: 'toggle'): void }>();

const { t } = useI18n();
</script>

<style scoped>
/*
 * §7.3 B 思考条目：与工具条目共用同一套 `FoldEntry` 行度量
 * （行高 26px、间距 8px、沟槽 20px、状态点 16px、箭头 12px）。
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
  background: transparent;
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

.tl-gutter-node {
  position: absolute;
  top: 9px;
  left: 7px;
  width: 6px;
  height: 6px;
  border-radius: 3px;
  background: rgba(62, 156, 126, 0.65);
}

.tl-gutter-node.is-loading {
  background: var(--mz-thought, #3e9c7e);
}

.tl-thinking-status {
  flex: 0 0 auto;
  width: 16px;
  height: 16px;
  box-sizing: border-box;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  font-size: 9px;
  line-height: 1;
  color: #ffffff;
  background: var(--mz-thought, #3e9c7e);
}

.tl-thinking-status.is-streaming {
  background: transparent;
  border: 2px solid rgba(62, 156, 126, 0.26);
  color: var(--mz-thought, #3e9c7e);
}

.tl-thinking-label {
  flex: 0 0 auto;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 13px;
}

.tl-thinking-summary {
  flex: 1 1 auto;
  min-width: 0;
  color: var(--mz-timeline-entry-soft, #9ba0a8);
  font-size: 12px;
  line-height: 1.5;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tl-thinking-toggle {
  flex: 0 0 auto;
  width: 12px;
  font-size: 10px;
  color: var(--mz-text-muted, #8a8f99);
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
