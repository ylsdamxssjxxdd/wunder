<template>
  <div class="cx-tip" :style="tipStyle" role="presentation">
    <div class="cx-tip-head">
      <span class="cx-tip-dot" :style="{ background: dotColor }"></span>
      <span class="cx-tip-title">{{ title }}</span>
      <span v-if="tip.kind !== 'item'" class="cx-tip-head-value">
        {{ tip.kind === 'part' ? `${tip.value} · ${tip.pct}` : tip.value }}
      </span>
    </div>
    <div v-if="tip.kind === 'item'" class="cx-tip-sub">
      {{ partLabelText }} · {{ tip.value }} token · {{ tip.pct }}
    </div>
    <div v-for="(row, index) in tip.rows" :key="index" class="cx-tip-row">
      <span class="cx-tip-row-name" :class="{ 'is-on': row.on }">{{ rowLabel(row) }}</span>
      <span class="cx-tip-row-value" :class="{ 'is-on': row.on }">{{ row.value }}</span>
    </div>
    <div v-if="tip.more > 0" class="cx-tip-more">
      {{ t('chat.contextWindow.moreItems', { count: tip.more }) }}
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { useI18n } from '@/i18n';
import {
  contextItemLabel,
  paletteFor,
  partLabel,
  type ContextTip
} from '@/components/chat/contextWindowView';

const props = defineProps<{
  tip: ContextTip;
  /** Highlighted block rectangle, in map fractions. */
  rect: { x: number; y: number; w: number; h: number };
  mapWidth: number;
  mapHeight: number;
}>();

const { t } = useI18n();

const dotColor = computed(() =>
  props.tip.kind === 'free' ? 'var(--mz-text-muted, #8a8f99)' : paletteFor(props.tip.slot).ink
);

const title = computed(() => {
  if (props.tip.kind === 'free') return t('chat.contextWindow.free');
  if (props.tip.kind === 'part') return partLabel(props.tip.slot, t);
  return contextItemLabel(props.tip, t);
});

const partLabelText = computed(() => partLabel(props.tip.slot, t));

const rowLabel = (row: ContextTip['rows'][number]): string =>
  contextItemLabel(row, t);

// Mirror the desktop tip height formula so the card can flip above/below
// before it has laid out.
const tipHeight = computed(() => {
  const rows = props.tip.rows.length;
  const itemSub = props.tip.kind === 'item' ? 16 : 0;
  const moreRow = props.tip.more > 0 ? 14 : 0;
  const gapCount = rows + (props.tip.kind === 'item' ? 1 : 0) + (props.tip.more > 0 ? 1 : 0);
  return 16 + 18 + itemSub + rows * 15 + moreRow + gapCount * 2;
});

const tipStyle = computed(() => {
  const px = props.rect.x * props.mapWidth;
  const py = props.rect.y * props.mapHeight;
  const pw = props.rect.w * props.mapWidth;
  const ph = props.rect.h * props.mapHeight;
  const width = 240;
  const centerX = px + pw / 2;
  const left = Math.min(Math.max(centerX - width / 2, 4), props.mapWidth - width - 4);
  const above = py - tipHeight.value - 8 >= 4;
  const top = above ? py - tipHeight.value - 8 : py + ph + 8;
  return {
    left: `${left}px`,
    top: `${top}px`,
    width: `${width}px`,
    minHeight: `${tipHeight.value}px`
  };
});
</script>

<style scoped>
.cx-tip {
  position: absolute;
  z-index: 20;
  box-sizing: border-box;
  padding: 8px 10px;
  border-radius: 9px;
  background: #17171b;
  box-shadow: 0 10px 30px rgba(0, 0, 0, 0.28);
  color: #ffffff;
  pointer-events: none;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.cx-tip-head {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 18px;
}

.cx-tip-dot {
  flex: 0 0 auto;
  width: 6px;
  height: 6px;
  border-radius: 3px;
}

.cx-tip-title {
  flex: 1 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  font-weight: 600;
}

.cx-tip-head-value {
  flex: 0 0 auto;
  color: #a1a1aa;
  font-size: 11px;
}

.cx-tip-sub {
  height: 16px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: #a1a1aa;
  font-size: 11px;
  line-height: 16px;
}

.cx-tip-row {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 15px;
}

.cx-tip-row-name {
  flex: 1 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: #b9b9c1;
  font-size: 11px;
}

.cx-tip-row-value {
  flex: 0 0 auto;
  color: #b9b9c1;
  font-size: 11px;
  font-weight: 400;
  font-variant-numeric: tabular-nums;
}

.cx-tip-row-name.is-on,
.cx-tip-row-value.is-on {
  color: #ffffff;
}

.cx-tip-row-value.is-on {
  font-weight: 600;
}

.cx-tip-more {
  height: 14px;
  color: #85858d;
  font-size: 11px;
  line-height: 14px;
}
</style>
