<template>
  <el-dialog
    :model-value="open"
    class="context-window-card"
    :show-close="false"
    append-to-body
    destroy-on-close
    align-center
    @update:model-value="handleVisibleChange"
  >
    <div class="cx-card">
      <!-- Head: title, model, badge, close -->
      <div class="cx-head">
        <span class="cx-title">{{ t('chat.contextWindow.title') }}</span>
        <span class="cx-head-stretch"></span>
        <template v-if="cardWidth >= 480 && modelLabelText">
          <i class="fa-solid fa-cubes cx-head-cubes" aria-hidden="true"></i>
          <span class="cx-head-model">{{ modelLabelText }}</span>
        </template>
        <span v-if="cardWidth >= 340 && hasData && !unobserved" class="cx-badge">
          {{ record?.measured ? t('chat.contextWindow.measured') : t('chat.contextWindow.estimate') }}
        </span>
        <button
          class="cx-close"
          type="button"
          :aria-label="t('common.close')"
          @click="emit('update:open', false)"
        >
          <i class="fa-solid fa-xmark" aria-hidden="true"></i>
        </button>
      </div>

      <div v-if="loading" class="cx-state">
        <span class="cx-spinner" aria-hidden="true"></span>
        <span>{{ t('common.loading') }}</span>
      </div>

      <div v-else-if="loadError" class="cx-state cx-state--error">{{ loadError }}</div>

      <template v-else-if="record">
        <!-- Occupancy: big number, share and remainder -->
        <div v-if="hasData && !unobserved" class="cx-occupancy">
          <div class="cx-occupancy-main">
            <span class="cx-used">{{ usedText }}</span>
            <span class="cx-capacity">/ {{ capacityText }} token</span>
          </div>
          <div class="cx-occupancy-sub">
            <span>{{ summaryText }}</span>
            <span v-if="record.turns > 0" class="cx-turns">
              · {{ t('chat.contextWindow.turns', { count: record.turns }) }}
            </span>
          </div>
        </div>

        <div v-if="unobserved" class="cx-note">{{ t('chat.contextWindow.unobserved') }}</div>

        <template v-else-if="hasData">
          <!-- Window map: two-level treemap, fills the remaining card space -->
          <div
            ref="mapRef"
            class="cx-map"
            @mouseleave="onLeaveMap"
          >
            <button
              v-for="(tile, index) in laid.tiles"
              :key="`${tile.slot}-${tile.kind}-${tile.item}-${index}`"
              class="cx-tile"
              :class="{ 'is-hatch': tile.kind === 'free' }"
              :style="tileStyle(tile, index)"
              type="button"
              @mouseenter="onTileEnter(index)"
              @click="onTileClick(index)"
            >
              <template v-if="tile.showName">
                <span class="cx-tile-label" :style="{ color: tileLabelColor(tile), width: `${tileTextWidth(tile)}px` }">{{ tileLabel(tile) }}</span>
                <span v-if="tile.showValue" class="cx-tile-value" :style="{ color: paletteFor(tile.slot).value, width: `${tileTextWidth(tile)}px` }">
                  {{ tileValue(tile) }}
                </span>
              </template>
            </button>

            <ContextHoverTip
              v-if="tip && tipRect"
              :tip="tip"
              :rect="tipRect"
              :map-width="mapWidth"
              :map-height="mapHeight"
            />
          </div>

          <ContextWindowLegend
            :legend="legend"
            :free-text="freeText"
            :free="freeTokens > 0"
            :wide="cardWidth >= 640"
            @legend-click="onLegendClick"
            @legend-enter="onLegendEnter"
            @legend-free-enter="onLegendFreeEnter"
          />
        </template>

        <div v-else class="cx-state cx-state--empty">{{ t('chat.contextWindow.noRequest') }}</div>
      </template>
    </div>

    <ContextItemDetail
      v-if="detail?.kind === 'item'"
      :state="detail"
      :part-label="partLabel(detail.slot, t)"
      @close="closeDetail"
    />
    <ContextPartDetail
      v-else-if="detail?.kind === 'part'"
      :state="detail"
      @close="closeDetail"
      @toggle-row="togglePartRow"
    />
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, type ComputedRef } from 'vue';

import { getSessionContextWindow, getSessionContextWindowText } from '@/api/chat';
import { useI18n } from '@/i18n';
import {
  contextItemLabel,
  findListedItem,
  fmtK,
  hasContextData,
  layoutTiles,
  legendRows,
  modelLabel,
  normalizeContextWindow,
  paletteFor,
  partDetailRows,
  partLabel,
  pctText,
  tileTarget,
  type ContextDetailRow,
  type ContextTarget,
  type ContextTip,
  type ContextTile,
  type ContextWindowRecord,
  type DetailState,
  type ContextLegendRow
} from '@/components/chat/contextWindowView';
import ContextHoverTip from '@/components/chat/ContextHoverTip.vue';
import ContextItemDetail from '@/components/chat/ContextItemDetail.vue';
import ContextPartDetail from '@/components/chat/ContextPartDetail.vue';
import ContextWindowLegend from '@/components/chat/ContextWindowLegend.vue';

const props = defineProps<{
  open: boolean;
  sessionId: string;
  /** Whether the current occupancy has an observed provider count. */
  observed?: boolean;
  /** The thread's history has been replaced by a compaction summary. */
  compacted?: boolean;
}>();

const emit = defineEmits<{ 'update:open': [value: boolean] }>();

const { t } = useI18n();

const record = ref<ContextWindowRecord | null>(null);
const loading = ref(false);
const loadError = ref('');
const detail = ref<DetailState | null>(null);

const focusSlot = ref(0);
const focusItem = ref(-1);
const hoveredTile = ref(-1);
const tip = ref<ContextTip | null>(null);
const tipRect = ref<{ x: number; y: number; w: number; h: number } | null>(null);

// The dialog body is re-created on every open (destroy-on-close), so sizing
// is derived from the viewport and the fixed CSS rules instead of element
// observers that would miss the lazily mounted slots.
const viewport = ref({
  w: typeof window === 'undefined' ? 1280 : window.innerWidth,
  h: typeof window === 'undefined' ? 900 : window.innerHeight
});

const onViewportResize = (): void => {
  viewport.value = { w: window.innerWidth, h: window.innerHeight };
};

const narrowViewport = computed(() => viewport.value.w <= 420);
const cardWidth = computed(() =>
  Math.min(920, viewport.value.w - (narrowViewport.value ? 16 : 24))
);

const mapRef = ref<HTMLElement | null>(null);
const mapWidth = ref(880);
const mapHeight = ref(300);

let mapObserver: ResizeObserver | null = null;
let loadController: AbortController | null = null;
let textController: AbortController | null = null;

const unobserved = computed(() => Boolean(props.compacted) && !props.observed);
const hasData = computed(() => (record.value ? hasContextData(record.value) : false));

const usedText = computed(() => {
  if (!record.value || !hasData.value || unobserved.value) return '—';
  return fmtK(record.value.tokens);
});

const capacityText = computed(() => {
  if (!record.value || record.value.window <= 0) return '—';
  return fmtK(record.value.window);
});

const freeTokens = computed(() => {
  if (!record.value) return 0;
  return Math.max(0, Math.max(record.value.window, record.value.tokens) - record.value.tokens);
});

const freeText = computed(() => fmtK(freeTokens.value));

const summaryText = computed(() => {
  if (!record.value || !hasData.value || unobserved.value || record.value.window <= 0) return '—';
  return t('chat.contextWindow.summary', {
    pct: pctText(record.value.tokens, record.value.window),
    free: freeText.value
  });
});

const modelLabelText = computed(() => (record.value ? modelLabel(record.value) : ''));

const legend: ComputedRef<ContextLegendRow[]> = computed(() =>
  record.value ? legendRows(record.value, t) : []
);

const laid = computed(() =>
  record.value ? layoutTiles(record.value, mapWidth.value, mapHeight.value) : { tiles: [], tips: [] }
);

// ---- Loading -------------------------------------------------------------

const load = async (): Promise<void> => {
  loadController?.abort();
  const controller = new AbortController();
  loadController = controller;
  loading.value = true;
  detail.value = null;
  try {
    if (!props.sessionId.trim()) {
      record.value = null;
      return;
    }
    const response = await getSessionContextWindow(props.sessionId, { signal: controller.signal });
    record.value = normalizeContextWindow(response.data?.data);
  } catch (error) {
    if (controller.signal.aborted) return;
    record.value = null;
    loadError.value = resolveErrorMessage(error);
  } finally {
    if (!controller.signal.aborted) loading.value = false;
  }
};

const resolveErrorMessage = (error: unknown): string => {
  const source = error as { response?: { status?: number }; message?: string };
  if (source?.response?.status === 404) return t('chat.contextWindow.unavailable');
  return source?.message || t('chat.contextWindow.loadError');
};

// ---- Map hover ------------------------------------------------------------

const clearMapFocus = (): void => {
  hoveredTile.value = -1;
  focusSlot.value = 0;
  focusItem.value = -1;
  tip.value = null;
  tipRect.value = null;
};

const onLeaveMap = (): void => {
  clearMapFocus();
};

const onTileEnter = (index: number): void => {
  const tile = laid.value.tiles[index];
  if (!tile) return;
  hoveredTile.value = index;
  focusSlot.value = tile.kind === 'free' ? -1 : tile.slot;
  focusItem.value = tile.kind === 'item' ? tile.item : -1;
  tip.value = laid.value.tips[index] ?? null;
  tipRect.value = { x: tile.x, y: tile.y, w: tile.w, h: tile.h };
};

const onLegendEnter = (slot: number): void => {
  focusSlot.value = slot;
  focusItem.value = -1;
};

const onLegendFreeEnter = (): void => {
  focusSlot.value = -1;
  focusItem.value = -1;
};

// ---- Click targets ---------------------------------------------------------

const onTileClick = (index: number): void => {
  const tile = laid.value.tiles[index];
  if (!tile || !record.value) return;
  const target = tileTarget(record.value, tile);
  if (target) openTarget(target);
};

const onLegendClick = (slot: number): void => {
  clearMapFocus();
  openPart(slot);
};

const openTarget = (target: ContextTarget): void => {
  if (target.kind === 'part') openPart(target.slot);
  else void openItem(target);
};

const openItem = async (target: { slot: number; tag: string; name: string; count: number }): Promise<void> => {
  if (!record.value) return;
  const part = record.value.parts.find((entry) => entry.slot === target.slot);
  const item = part ? findListedItem(part, target.tag, target.name, target.count) : undefined;
  if (!part || !item) return;
  textController?.abort();
  const controller = new AbortController();
  textController = controller;
  detail.value = {
    kind: 'item',
    slot: part.slot,
    tag: item.tag,
    name: item.name,
    count: item.count,
    label: contextItemLabel(item, t),
    tokensText: fmtK(item.tokens),
    pctText: pctText(item.tokens, record.value.tokens),
    text: '',
    loading: true,
    unavailable: false
  };
  try {
    const response = await getSessionContextWindowText(
      props.sessionId,
      { slot: part.slot, tag: item.tag, name: item.name, count: item.count },
      { signal: controller.signal }
    );
    if (controller.signal.aborted) return;
    const body = response.data?.data as { text?: unknown } | null | undefined;
    if (detail.value?.kind === 'item') {
      detail.value = { ...detail.value, text: String(body?.text ?? ''), loading: false };
    }
  } catch (error) {
    if (controller.signal.aborted) return;
    if (detail.value?.kind === 'item') {
      detail.value = { ...detail.value, loading: false, unavailable: true };
    }
  }
};

const openPart = (slot: number): void => {
  if (!record.value) return;
  const part = record.value.parts.find((entry) => entry.slot === slot);
  if (!part) return;
  detail.value = {
    kind: 'part',
    slot,
    label: partLabel(slot, t),
    tokensText: fmtK(part.tokens),
    pctText: pctText(part.tokens, record.value.tokens),
    rows: partDetailRows(record.value, slot, t),
    openRow: -1,
    rowText: '',
    rowLoading: false
  };
};

const togglePartRow = async (row: ContextDetailRow): Promise<void> => {
  if (!detail.value || detail.value.kind !== 'part') return;
  if (detail.value.openRow === row.item) {
    detail.value = { ...detail.value, openRow: -1, rowText: '', rowLoading: false };
    return;
  }
  textController?.abort();
  const controller = new AbortController();
  textController = controller;
  detail.value = { ...detail.value, openRow: row.item, rowText: '', rowLoading: true };
  try {
    const response = await getSessionContextWindowText(
      props.sessionId,
      { slot: row.slot, tag: row.tag, name: row.name, count: row.count },
      { signal: controller.signal }
    );
    if (controller.signal.aborted) return;
    const body = response.data?.data as { text?: unknown } | null | undefined;
    if (detail.value?.kind === 'part') {
      detail.value = { ...detail.value, rowText: String(body?.text ?? ''), rowLoading: false };
    }
  } catch {
    if (!controller.signal.aborted && detail.value?.kind === 'part') {
      detail.value = { ...detail.value, rowText: '', rowLoading: false };
    }
  }
};

const closeDetail = (): void => {
  textController?.abort();
  clearMapFocus();
  detail.value = null;
};

// ---- Tile rendering helpers -----------------------------------------------

const tileLabel = (tile: ContextTile): string => {
  if (tile.kind === 'free') return t('chat.contextWindow.free');
  if (tile.kind === 'part') return partLabel(tile.slot, t);
  if (tile.kind === 'more') return t('chat.contextWindow.moreItems', { count: tile.count });
  return contextItemLabel(tile, t);
};

const tileLabelColor = (tile: ContextTile): string =>
  tile.kind === 'free' ? 'var(--mz-text-muted, #8a8f99)' : paletteFor(tile.slot).name;

const tileValue = (tile: ContextTile): string => {
  if (tile.kind === 'item' || tile.kind === 'more') return `${tile.tokensText} · ${partLabel(tile.slot, t)}`;
  if (tile.kind === 'free') {
    const scale = Math.max(1, Math.max(record.value?.window ?? 0, record.value?.tokens ?? 0));
    return `${tile.tokensText} · ${pctText(tile.tokens, scale)}`;
  }
  return tile.tokensText;
};

const tileOpacity = (tile: ContextTile, index: number): number => {
  if (hoveredTile.value >= 0) return hoveredTile.value === index ? 1 : 0.55;
  if (focusSlot.value === 0) return 1;
  if (tile.kind === 'free') return focusSlot.value === -1 ? 1 : 0.55;
  if (tile.slot === focusSlot.value) {
    if (focusItem.value < 0 || tile.kind === 'part' || tile.item === focusItem.value) return 1;
  }
  return 0.55;
};

// Inner text width follows the measured tile rect minus the 9px horizontal
// padding on each side (desktop slint rule); no parent flex shrink chain.
const tileTextWidth = (tile: ContextTile): number => {
  const width = Math.max(0, tile.w * mapWidth.value - 3);
  return Math.max(0, Math.round(width - 18));
};

const tileStyle = (tile: ContextTile, index: number): Record<string, string> => {
  const palette = paletteFor(tile.slot);
  const hover = hoveredTile.value === index;
  const left = tile.x * mapWidth.value + 1.5;
  const top = tile.y * mapHeight.value + 1.5;
  const width = Math.max(0, tile.w * mapWidth.value - 3);
  const height = Math.max(0, tile.h * mapHeight.value - 3);
  return {
    left: `${left}px`,
    top: `${top}px`,
    width: `${width}px`,
    height: `${height}px`,
    paddingTop: tile.showValue ? '7px' : '5px',
    paddingBottom: tile.showValue ? '7px' : '5px',
    backgroundColor: tile.kind === 'free' ? '#FAFBFC' : hover ? palette.hoverFill : palette.fill,
    borderColor: tile.kind === 'free' ? '#E6E8EB' : hover ? palette.ink : palette.line,
    opacity: String(tileOpacity(tile, index)),
    zIndex: hover ? '3' : ''
  };
};

const handleVisibleChange = (value: boolean): void => {
  emit('update:open', value);
};

onMounted(() => {
  window.addEventListener('resize', onViewportResize);
});

// The dialog body is re-created on each open, so observe the map once its
// ref binds again (lazy mount); the reported client width and height drive
// tile layout, and the map stretches to fill the remaining card space.
watch(mapRef, (el) => {
  mapObserver?.disconnect();
  if (!el) return;
  mapObserver = new ResizeObserver(() => {
    if (el.clientWidth > 0) mapWidth.value = el.clientWidth;
    if (el.clientHeight > 0) mapHeight.value = el.clientHeight;
  });
  mapObserver.observe(el);
  if (el.clientWidth > 0) mapWidth.value = el.clientWidth;
  if (el.clientHeight > 0) mapHeight.value = el.clientHeight;
});

onBeforeUnmount(() => {
  window.removeEventListener('resize', onViewportResize);
  mapObserver?.disconnect();
  loadController?.abort();
  textController?.abort();
});

watch(
  () => props.open,
  (value) => {
    if (value) {
      void load();
    } else {
      loadController?.abort();
      textController?.abort();
      detail.value = null;
      record.value = null;
      onLeaveMap();
    }
  }
);
</script>

<style scoped>
.cx-card {
  display: flex;
  flex-direction: column;
  gap: 10px;
  box-sizing: border-box;
  height: 100%;
  padding: 20px;
  overflow: hidden;
  background-color: var(--mz-surface, #ffffff);
}

.cx-head {
  display: flex;
  align-items: center;
  flex-wrap: nowrap;
  gap: 6px;
  height: 24px;
  min-width: 0;
}

.cx-title {
  flex: 0 0 auto;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  color: var(--mz-text, #1f2329);
  font-size: 15px;
  font-weight: 700;
}

.cx-head-stretch {
  flex: 1 1 auto;
}

.cx-head-cubes {
  color: var(--mz-text-muted, #8a8f99);
  font-size: 14px;
}

.cx-head-model {
  min-width: 0;
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--mz-text-secondary, #3d3d3d);
  font-size: 12px;
}

.cx-badge {
  height: 17px;
  padding: 0 8px;
  border-radius: 9px;
  background: var(--mz-hover, #f1efec);
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
  font-weight: 600;
  line-height: 17px;
}

.cx-close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
  cursor: pointer;
}

.cx-close:hover {
  background: var(--mz-hover, #f1efec);
}

.cx-occupancy {
  display: flex;
  flex-direction: column;
  flex: 0 0 auto;
  gap: 2px;
}

.cx-occupancy-main {
  display: flex;
  align-items: baseline;
  gap: 6px;
  height: 34px;
}

.cx-used {
  color: var(--mz-text, #1f2329);
  font-size: 26px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.cx-capacity {
  color: var(--mz-text-muted, #8a8f99);
  font-size: 13px;
}

.cx-occupancy-sub {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 16px;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
}

.cx-turns {
  color: var(--mz-text-muted, #8a8f99);
}

.cx-note {
  padding: 8px 10px;
  border-radius: 8px;
  background: var(--mz-hover, #f1efec);
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
}

.cx-map {
  position: relative;
  width: 100%;
  flex: 1 1 auto;
  min-height: 240px;
  background-color: var(--mz-surface, #ffffff);
}

.cx-tile {
  position: absolute;
  box-sizing: border-box;
  padding: 5px 9px;
  border: 1px solid;
  border-radius: 7px;
  overflow: hidden;
  text-align: left;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 1px;
}

.cx-tile.is-hatch {
  background-image: repeating-linear-gradient(
    45deg,
    rgba(31, 35, 41, 0.05) 0,
    rgba(31, 35, 41, 0.05) 1px,
    transparent 1px,
    transparent 6px
  );
}

.cx-tile-label {
  display: block;
  min-width: 0;
  width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11px;
  font-weight: 600;
}

.cx-tile-value {
  display: block;
  min-width: 0;
  width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 10px;
  font-weight: 400;
  font-variant-numeric: tabular-nums;
}

.cx-state {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 24px 12px;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 13px;
  text-align: center;
}

.cx-state--error {
  color: var(--mz-danger, #d04a43);
}

.cx-state--empty {
  height: 32px;
  padding: 0;
}

.cx-spinner {
  width: 12px;
  height: 12px;
  border: 2px solid rgba(31, 35, 41, 0.14);
  border-top-color: var(--mz-primary, #f97316);
  border-radius: 50%;
  animation: cx-spin 640ms linear infinite;
}

@keyframes cx-spin {
  to { transform: rotate(360deg); }
}
</style>

<!-- Dialog shell is teleported to body, so its overrides must live in a
     non-scoped block; the inner `.cx-*` classes stay scoped above. -->
<style>
.context-window-card.el-dialog {
  width: min(920px, calc(100vw - 24px));
  height: min(720px, calc(100vh - 24px));
  max-width: min(920px, calc(100vw - 24px));
  max-height: min(720px, calc(100vh - 24px));
  margin: 0 auto;
  top: 50%;
  transform: translateY(-50%);
  overflow: hidden;
  --el-dialog-bg-color: #ffffff;
  background-color: var(--mz-surface, #ffffff);
}

.context-window-card.el-dialog .el-dialog__header {
  display: none;
}

.context-window-card.el-dialog .el-dialog__body {
  height: 100%;
  padding: 0;
  overflow: hidden;
  background: var(--mz-surface, #ffffff);
}

@media (max-width: 420px) {
  .context-window-card.el-dialog {
    width: calc(100vw - 16px);
    height: calc(100vh - 16px);
    max-width: calc(100vw - 16px);
    max-height: calc(100vh - 16px);
  }
}
</style>
