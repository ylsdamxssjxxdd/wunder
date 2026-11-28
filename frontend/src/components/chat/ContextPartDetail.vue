<template>
  <div class="cx-detail-overlay" @mousedown.self="emit('close')">
    <div class="cx-detail-card cx-detail-card--part" :style="cardStyle" role="dialog" aria-modal="true">
      <div class="cx-detail-head">
        <span class="cx-detail-dot" :style="{ background: palette.ink }"></span>
        <span class="cx-detail-title">{{ state.label }}</span>
        <button class="cx-detail-close" type="button" :aria-label="t('common.close')" @click="emit('close')">
          <i class="fa-solid fa-xmark" aria-hidden="true"></i>
        </button>
      </div>
      <div class="cx-detail-sub">{{ state.tokensText }} token · {{ state.pctText }}</div>
      <div class="cx-detail-divider"></div>
      <div class="cx-detail-body">
        <div v-for="row in state.rows" :key="`${row.tag}-${row.name}-${row.count}-${row.item}`" class="cx-part-row">
          <button
            class="cx-part-row-main"
            type="button"
            :class="{ 'is-open': state.openRow === row.item }"
            @click="emit('toggle-row', row)"
          >
            <i
              :class="[
                'fa-solid',
                state.openRow === row.item ? 'fa-chevron-down' : 'fa-chevron-right'
              ]"
              class="cx-part-row-chevron"
              aria-hidden="true"
            ></i>
            <span class="cx-part-row-dot" :style="{ background: paletteFor(row.slot).ink }"></span>
            <span class="cx-part-row-label">{{ row.label }}</span>
            <span v-if="showCount(row)" class="cx-part-row-count">×{{ row.count }}</span>
            <span class="cx-part-row-bar">
              <span class="cx-part-row-bar-fill" :style="{ width: barWidth(row) }"></span>
            </span>
            <span class="cx-part-row-value">{{ row.tokensText }}</span>
          </button>
          <div v-if="state.openRow === row.item" class="cx-part-row-body">
            <div v-if="state.rowLoading" class="cx-detail-note">{{ t('common.loading') }}</div>
            <div v-else-if="state.rowText === ''" class="cx-detail-note">
              {{ t('chat.contextWindow.textReleased') }}
            </div>
            <pre v-else class="cx-detail-text">{{ state.rowText }}</pre>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { useI18n } from '@/i18n';
import {
  paletteFor,
  type ContextDetailRow,
  type PartDetailState
} from '@/components/chat/contextWindowView';

const props = defineProps<{ state: PartDetailState }>();

const emit = defineEmits<{
  close: [];
  'toggle-row': [row: ContextDetailRow];
}>();

const { t } = useI18n();

const palette = computed(() => paletteFor(props.state.slot));

const cardStyle = computed(() => {
  const byRows = 150 + props.state.rows.length * 34;
  const height = Math.min(520, Math.max(260, byRows));
  return { height: `${height}px` };
});

const showCount = (row: ContextDetailRow): boolean =>
  row.count > 1 && (row.tag === 'tool' || row.tag === 'result');

const barWidth = (row: ContextDetailRow): string => `${Math.max(2, Math.round(72 * row.ratio))}px`;
</script>

<style scoped>
.cx-detail-overlay {
  position: fixed;
  inset: 0;
  z-index: 2100;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 12px;
  background: rgba(15, 18, 22, 0.35);
}

.cx-detail-card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  box-sizing: border-box;
  width: min(560px, calc(100vw - 24px));
  padding: 20px;
  border: 1px solid var(--mz-border, #e8e6e3);
  border-radius: 12px;
  background: var(--mz-surface, #ffffff);
  box-shadow: 0 16px 48px rgba(15, 18, 22, 0.24);
}

.cx-detail-head {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 24px;
}

.cx-detail-dot {
  flex: 0 0 auto;
  width: 8px;
  height: 8px;
  border-radius: 4px;
}

.cx-detail-title {
  flex: 1 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 15px;
  font-weight: 700;
  color: var(--mz-text, #1f2329);
}

.cx-detail-close {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 16px;
  cursor: pointer;
}

.cx-detail-close:hover {
  background: var(--mz-hover, #f1efec);
}

.cx-detail-sub {
  height: 18px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
  line-height: 18px;
}

.cx-detail-divider {
  height: 1px;
  background: var(--mz-border, #e8e6e3);
}

.cx-detail-body {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.cx-part-row-main {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  height: 30px;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--mz-text, #1f2329);
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.cx-part-row-main:hover,
.cx-part-row-main.is-open {
  background: var(--mz-hover, #f1efec);
}

.cx-part-row-chevron {
  flex: 0 0 auto;
  width: 9px;
  margin-left: 7px;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 9px;
}

.cx-part-row-dot {
  flex: 0 0 auto;
  width: 6px;
  height: 6px;
  margin-left: 6px;
  border-radius: 3px;
}

.cx-part-row-label {
  flex: 1 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
  font-weight: 500;
}

.cx-part-row-count {
  flex: 0 0 auto;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
}

.cx-part-row-bar {
  flex: 0 0 auto;
  width: 72px;
  height: 4px;
  border-radius: 2px;
  background: var(--mz-selected, #edeae6);
  overflow: hidden;
}

.cx-part-row-bar-fill {
  display: block;
  height: 100%;
  border-radius: 2px;
}

.cx-part-row-value {
  flex: 0 0 auto;
  width: 72px;
  text-align: right;
  color: var(--mz-text-secondary, #3d3d3d);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}

.cx-part-row-body {
  padding: 0 2px;
}

.cx-detail-note {
  padding: 6px 8px;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
  line-height: 1.6;
}

.cx-detail-text {
  margin: 6px 0;
  padding: 8px 10px;
  border-radius: 6px;
  background: var(--mz-hover, #f1efec);
  color: var(--mz-text, #1f2329);
  font-family: "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace;
  font-size: 11.5px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-word;
}
</style>
