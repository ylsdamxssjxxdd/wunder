<script setup lang="ts">
import { nextTick, ref, watch } from 'vue';
import { useI18n } from '@/i18n';
import { paletteFor, type ContextLegendRow, type ContextListRow } from './contextWindowView';

const props = defineProps<{
  rows: ContextListRow[];
  legend: ContextLegendRow[];
  filterSlot: number;
  /** Wide enough to show the 72px mini bar; hidden below 420px. */
  wide: boolean;
}>();

const emit = defineEmits<{
  'select-slot': [slot: number];
  'row-click': [row: ContextListRow];
  'row-enter': [row: ContextListRow];
  'row-leave': [];
}>();

const { t } = useI18n();

const listEl = ref<HTMLElement | null>(null);

// Opening the card or switching the filter always lands on the first row; the
// scroll container must not keep the previous list's offset.
watch(
  () => [props.rows, props.filterSlot],
  async () => {
    await nextTick();
    if (listEl.value) listEl.value.scrollTop = 0;
  }
);

// The mini bar's share is relative to the largest row in the current list,
// not to the window.
const barHint = t('chat.contextWindow.barRelativeMax');
</script>

<template>
  <div class="cx-list-panel">
    <div class="cx-contents-head">
      <span class="cx-contents-title">{{ t('chat.contextWindow.contents') }}</span>
      <div class="cx-tabs" role="tablist">
        <button
          class="cx-tab"
          :class="{ 'is-active': filterSlot === 0 }"
          type="button"
          @click="emit('select-slot', 0)"
        >
          {{ t('chat.contextWindow.all') }}
        </button>
        <button
          v-for="entry in legend"
          :key="entry.slot"
          class="cx-tab"
          :class="{ 'is-active': filterSlot === entry.slot }"
          type="button"
          @click="emit('select-slot', entry.slot)"
        >
          {{ entry.label }}
        </button>
      </div>
    </div>

    <div
      ref="listEl"
      class="cx-list"
      role="list"
      tabindex="0"
      @mouseleave="emit('row-leave')"
    >
      <button
        v-for="row in rows"
        :key="`${row.slot}-${row.tag}-${row.name}-${row.count}-${row.order}`"
        class="cx-row"
        :class="{ 'is-narrow': !wide }"
        type="button"
        role="listitem"
        @mouseenter="emit('row-enter', row)"
        @click="emit('row-click', row)"
      >
        <span class="cx-row-dot" :style="{ background: paletteFor(row.slot).ink }"></span>
        <span class="cx-row-label">{{ row.label }}</span>
        <span v-if="row.count > 1 && (row.tag === 'tool' || row.tag === 'result')" class="cx-row-count">×{{ row.count }}</span>
        <span
          v-if="wide"
          class="cx-row-bar"
          :title="barHint"
          :aria-label="`${barHint}: ${row.tokensText}`"
        >
          <span
            class="cx-row-bar-fill"
            :style="{ width: `${Math.max(2, Math.round(72 * row.ratio))}px`, background: paletteFor(row.slot).ink }"
          ></span>
        </span>
        <span class="cx-row-value">{{ row.tokensText }}</span>
      </button>
      <div v-if="rows.length === 0" class="cx-state cx-state--empty">{{ t('chat.contextWindow.nothing') }}</div>
    </div>
  </div>
</template>

<style scoped>
.cx-list-panel {
  display: flex;
  flex-direction: column;
  flex: 1 1 auto;
  min-height: 0;
  overflow: hidden;
}

.cx-contents-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 6px 8px;
  min-height: 26px;
}

.cx-contents-title {
  flex: 0 0 auto;
  color: var(--mz-text, #1f2329);
  font-size: 12px;
  font-weight: 600;
}

.cx-tabs {
  display: flex;
  flex-wrap: nowrap;
  gap: 2px;
  max-width: 100%;
  height: 24px;
  padding: 2px;
  border-radius: 12px;
  background: var(--mz-hover, #f1efec);
}

.cx-tab {
  height: 20px;
  padding: 0 8px;
  border: 0;
  border-radius: 10px;
  background: transparent;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
  font-weight: 600;
  white-space: nowrap;
  cursor: pointer;
}

.cx-tab.is-active {
  background: var(--mz-surface, #ffffff);
  border: 1px solid var(--mz-border, #e8e6e3);
  color: var(--mz-text, #1f2329);
}

.cx-list {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  padding-right: 12px;
  outline: none;
}

.cx-row {
  position: relative;
  width: 100%;
  height: 28px;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--mz-text, #1f2329);
  font: inherit;
  font-size: 12px;
  text-align: left;
  cursor: pointer;
}

.cx-row:hover {
  background: var(--mz-hover, #f1efec);
}

.cx-row-dot {
  position: absolute;
  left: 6px;
  top: 50%;
  width: 6px;
  height: 6px;
  transform: translateY(-50%);
  border-radius: 3px;
}

.cx-row-label {
  position: absolute;
  left: 18px;
  top: 0;
  height: 100%;
  width: calc(100% - 190px);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 500;
}

.cx-row.is-narrow .cx-row-label {
  width: calc(100% - 82px);
}

.cx-row-count {
  position: absolute;
  left: calc(100% - 178px);
  top: 0;
  height: 100%;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
}

.cx-row-bar {
  position: absolute;
  right: 150px;
  top: 50%;
  width: 72px;
  height: 4px;
  transform: translateY(-50%);
  border-radius: 2px;
  background: var(--mz-selected, #edeae6);
  overflow: hidden;
}

.cx-row-bar-fill {
  display: block;
  height: 100%;
  border-radius: 2px;
}

.cx-row-value {
  position: absolute;
  right: 72px;
  top: 0;
  width: 72px;
  height: 100%;
  text-align: right;
  color: var(--mz-text-secondary, #3d3d3d);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}

.cx-state--empty {
  height: 32px;
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
  text-align: center;
}
</style>
