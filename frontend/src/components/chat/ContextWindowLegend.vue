<script setup lang="ts">
import { useI18n } from '@/i18n';
import { paletteFor, type ContextLegendRow } from './contextWindowView';

defineProps<{
  legend: ContextLegendRow[];
  freeText: string;
  free: boolean;
  /** Show token values only on cards at least 640px wide. */
  wide: boolean;
}>();

const emit = defineEmits<{
  'legend-click': [slot: number];
  'legend-enter': [slot: number];
  'legend-free-enter': [];
}>();

const { t } = useI18n();
</script>

<template>
  <div class="cx-legend">
    <button
      v-for="entry in legend"
      :key="entry.slot"
      class="cx-legend-row"
      type="button"
      @mouseenter="emit('legend-enter', entry.slot)"
      @mouseleave="emit('legend-free-enter')"
      @click="emit('legend-click', entry.slot)"
    >
      <span class="cx-legend-dot" :style="{ background: paletteFor(entry.slot).ink }"></span>
      <span class="cx-legend-label">{{ entry.label }}</span>
      <span v-if="wide" class="cx-legend-value">{{ entry.tokensText }}</span>
    </button>
    <button
      v-if="free"
      class="cx-legend-row"
      type="button"
      @mouseenter="emit('legend-free-enter')"
      @mouseleave="emit('legend-free-enter')"
    >
      <span class="cx-legend-dot cx-legend-dot--free"></span>
      <span class="cx-legend-label">{{ t('chat.contextWindow.free') }}</span>
      <span v-if="wide" class="cx-legend-value">{{ freeText }}</span>
    </button>
  </div>
</template>

<style scoped>
.cx-legend {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  flex: 0 0 auto;
  gap: 6px 14px;
  min-height: 20px;
  min-width: 0;
  overflow: hidden;
}

.cx-legend-row {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  gap: 6px;
  height: 20px;
  padding: 0 4px;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: var(--mz-text-secondary, #3d3d3d);
  font-size: 11px;
  white-space: nowrap;
  cursor: pointer;
}

.cx-legend-row:hover {
  background: var(--mz-hover, #f1efec);
}

.cx-legend-dot {
  flex: 0 0 auto;
  width: 6px;
  height: 6px;
  border-radius: 3px;
}

.cx-legend-dot--free {
  background: var(--mz-border-strong, #d8d5d0);
}

.cx-legend-label {
  flex: 0 0 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cx-legend-value {
  flex: 0 0 auto;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}
</style>
