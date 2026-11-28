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
      <!-- Head: title, model, badge, view toggle, close -->
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
        <div class="cx-toggle" role="tablist" aria-label="view">
          <button
            class="cx-toggle-btn"
            :class="{ 'is-active': mapMode }"
            type="button"
            @click="mapMode = true"
          >
            <svg viewBox="0 0 12 12" aria-hidden="true"><rect x="0.6" y="5" width="2.4" height="6.5" rx="1.2" fill="currentColor"/><rect x="4.8" y="1.5" width="2.4" height="10" rx="1.2" fill="currentColor"/><rect x="9" y="4" width="2.4" height="7.5" rx="1.2" fill="currentColor"/></svg>
          </button>
          <button
            class="cx-toggle-btn"
            :class="{ 'is-active': !mapMode }"
            type="button"
            @click="mapMode = false"
          >
            <svg viewBox="0 0 12 12" aria-hidden="true"><rect x="0.5" y="0.5" width="5" height="5" rx="1.1" fill="currentColor"/><rect x="6.5" y="0.5" width="5" height="5" rx="1.1" fill="currentColor"/><rect x="0.5" y="6.5" width="5" height="5" rx="1.1" fill="currentColor"/><rect x="6.5" y="6.5" width="5" height="5" rx="1.1" fill="currentColor"/></svg>
          </button>
        </div>
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
          <!-- Window map: two-level treemap or 400-cell grid -->
          <div
            ref="mapRef"
            class="cx-map"
            :class="{ 'is-grid': !mapMode }"
            :style="mapStyle"
            @mouseleave="onLeaveMap"
          >
            <template v-if="mapMode">
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
                  <span class="cx-tile-label" :style="{ color: tileLabelColor(tile) }">{{ tileLabel(tile) }}</span>
                  <span v-if="tile.showValue" class="cx-tile-value" :style="{ color: paletteFor(tile.slot).value }">
                    {{ tileValue(tile) }}
                  </span>
                </template>
              </button>
            </template>
            <template v-else>
              <button
                v-for="(cell, index) in cells"
                :key="index"
                class="cx-cell"
                :style="cellStyle(cell, index)"
                type="button"
                @mouseenter="onCellEnter(index)"
                @click="onCellClick(index)"
              ></button>
            </template>

            <ContextHoverTip
              v-if="tip && tipRect"
              :tip="tip"
              :rect="tipRect"
              :map-width="mapWidth"
              :map-height="mapBoxHeight"
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

          <div class="cx-divider"></div>

          <!-- Contents: section head, filter capsule, internal-scroll list -->
          <ContextWindowList
            :rows="rows"
            :legend="legend"
            :filter-slot="filterSlot"
            :wide="cardWidth >= 420"
            @select-slot="filterSlot = $event"
            @row-click="onRowClick"
            @row-enter="onRowEnter"
            @row-leave="onRowLeave"
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
  gridCells,
  gridLayout,
  hasContextData,
  layoutTiles,
  legendRows,
  listRows,
  modelLabel,
  normalizeContextWindow,
  paletteFor,
  partDetailRows,
  partLabel,
  partTip,
  pctText,
  cellTarget,
  tileTarget,
  type ContextCell,
  type ContextDetailRow,
  type ContextListRow,
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
import ContextWindowList from '@/components/chat/ContextWindowList.vue';
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
const mapMode = ref(true);
const filterSlot = ref(0);
const detail = ref<DetailState | null>(null);

const focusSlot = ref(0);
const focusItem = ref(-1);
// Row hover is a separate focus: only the matching tile lights up; the rest of
// the map keeps its colour instead of being washed out to a faint overlay.
const rowFocus = ref<{ slot: number; rank: number } | null>(null);
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
const cardHeight = computed(() =>
  Math.min(720, viewport.value.h - (narrowViewport.value ? 16 : 24))
);
const mapRef = ref<HTMLElement | null>(null);
const mapWidth = ref(880);

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

const rows: ComputedRef<ContextListRow[]> = computed(() =>
  record.value ? listRows(record.value, filterSlot.value, t) : []
);

const laid = computed(() =>
  record.value ? layoutTiles(record.value, mapWidth.value, mapBoxHeight.value) : { tiles: [], tips: [] }
);

const cells = computed(() => (record.value ? gridCells(record.value) : []));

const grid = computed(() => gridLayout(cardWidth.value));
const gridSide = computed(() =>
  Math.floor((mapWidth.value - (grid.value.columns - 1) * 2) / grid.value.columns)
);
const gridHeight = computed(() =>
  grid.value.rows * gridSide.value + (grid.value.rows - 1) * 2
);
const gridPadX = computed(() =>
  Math.max(0, (mapWidth.value - (grid.value.columns * gridSide.value + (grid.value.columns - 1) * 2)) / 2)
);
const gridPadY = computed(() => Math.max(0, (gridHeight.value - grid.value.rows * gridSide.value - (grid.value.rows - 1) * 2) / 2));

const mapBoxHeight = computed(() => {
  if (!mapMode.value) return gridHeight.value;
  const ratio = cardWidth.value <= 520 ? 2 : 3.1;
  const byRatio = mapWidth.value / ratio;
  const byCard = Math.max(0, cardHeight.value - 300);
  return Math.min(232, Math.max(96, Math.min(byRatio, byCard)));
});

const mapStyle = computed(() => ({ height: `${mapBoxHeight.value}px` }));

// ---- Loading -------------------------------------------------------------

const load = async (): Promise<void> => {
  loadController?.abort();
  const controller = new AbortController();
  loadController = controller;
  loading.value = true;
  detail.value = null;
  filterSlot.value = 0;
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

// ---- Map / grid hover -----------------------------------------------------

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

const freeTip = (): ContextTip => ({
  kind: 'free',
  slot: 0,
  tag: '',
  name: '',
  count: 0,
  value: freeText.value,
  pct: pctText(freeTokens.value, Math.max(1, Math.max(record.value?.window ?? 0, record.value?.tokens ?? 0))),
  rows: [],
  more: 0
});

const cellRect = (index: number): { x: number; y: number; w: number; h: number } => {
  const col = index % grid.value.columns;
  const row = Math.floor(index / grid.value.columns);
  const x = gridPadX.value + col * (gridSide.value + 2);
  const y = gridPadY.value + row * (gridSide.value + 2);
  return {
    x: x / mapWidth.value,
    y: y / mapBoxHeight.value,
    w: gridSide.value / mapWidth.value,
    h: gridSide.value / mapBoxHeight.value
  };
};

const onCellEnter = (index: number): void => {
  const cell = cells.value[index];
  if (!cell || !record.value) return;
  if (cell.slot === 0) {
    focusSlot.value = -1;
    focusItem.value = -1;
    tip.value = freeTip();
    tipRect.value = cellRect(index);
    return;
  }
  const part = record.value.parts.find((entry) => entry.slot === cell.slot);
  if (!part) return;
  focusSlot.value = cell.slot;
  focusItem.value = cell.item;
  tip.value = partTip(record.value, part, cell.item);
  tipRect.value = cellRect(index);
};

const onLegendEnter = (slot: number): void => {
  focusSlot.value = slot;
  focusItem.value = -1;
};

const onLegendFreeEnter = (): void => {
  focusSlot.value = -1;
  focusItem.value = -1;
};

const onRowEnter = (row: ContextListRow): void => {
  clearMapFocus();
  rowFocus.value = { slot: row.slot, rank: row.rank };
};

const onRowLeave = (): void => {
  rowFocus.value = null;
};

// ---- Click targets ---------------------------------------------------------

const onTileClick = (index: number): void => {
  const tile = laid.value.tiles[index];
  if (!tile || !record.value) return;
  const target = tileTarget(record.value, tile);
  if (target) openTarget(target);
};

const onCellClick = (index: number): void => {
  const cell = cells.value[index];
  if (!cell || !record.value) return;
  const target = cellTarget(record.value, cell);
  if (target) openTarget(target);
};

const onRowClick = (row: ContextListRow): void => {
  rowFocus.value = null;
  clearMapFocus();
  openTarget({ kind: 'item', slot: row.slot, tag: row.tag, name: row.name, count: row.count, itemRank: row.rank });
};

const onLegendClick = (slot: number): void => {
  rowFocus.value = null;
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
  rowFocus.value = null;
  clearMapFocus();
  detail.value = null;
};

// ---- Tile / cell rendering helpers ----------------------------------------

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

const tileRowHit = (tile: ContextTile): boolean => {
  const focus = rowFocus.value;
  if (!focus || tile.slot !== focus.slot) return false;
  if (tile.kind === 'item') return tile.item === focus.rank;
  if (tile.kind === 'more') return tile.its.includes(focus.rank);
  return tile.kind === 'part';
};

const tileOpacity = (tile: ContextTile, index: number): number => {
  if (hoveredTile.value >= 0) return hoveredTile.value === index ? 1 : 0.55;
  if (rowFocus.value) return tileRowHit(tile) ? 1 : 0.9;
  if (focusSlot.value === 0) return 1;
  if (tile.kind === 'free') return focusSlot.value === -1 ? 1 : 0.55;
  if (tile.slot === focusSlot.value) {
    if (focusItem.value < 0 || tile.kind === 'part' || tile.item === focusItem.value) return 1;
  }
  return 0.55;
};

const tileStyle = (tile: ContextTile, index: number): Record<string, string> => {
  const palette = paletteFor(tile.slot);
  const hover = hoveredTile.value === index;
  const rowHit = tileRowHit(tile);
  const active = hover || rowHit;
  const left = tile.x * mapWidth.value + 1.5;
  const top = tile.y * mapBoxHeight.value + 1.5;
  const width = Math.max(0, tile.w * mapWidth.value - 3);
  const height = Math.max(0, tile.h * mapBoxHeight.value - 3);
  return {
    left: `${left}px`,
    top: `${top}px`,
    width: `${width}px`,
    height: `${height}px`,
    paddingTop: tile.showValue ? '7px' : '5px',
    paddingBottom: tile.showValue ? '7px' : '5px',
    backgroundColor: tile.kind === 'free' ? '#FAFBFC' : active ? palette.hoverFill : palette.fill,
    borderColor: tile.kind === 'free' ? '#E6E8EB' : active ? palette.ink : palette.line,
    opacity: String(tileOpacity(tile, index)),
    boxShadow: rowHit ? `0 0 0 2px ${palette.ink}, 0 0 0 6px ${palette.ink}29` : '',
    zIndex: rowHit ? '3' : ''
  };
};

const cellRowHit = (cell: ContextCell): boolean =>
  Boolean(rowFocus.value) &&
  cell.slot === rowFocus.value!.slot &&
  cell.item === rowFocus.value!.rank;

const cellStyle = (cell: ContextCell, index: number): Record<string, string> => {
  const col = index % grid.value.columns;
  const row = Math.floor(index / grid.value.columns);
  const left = gridPadX.value + col * (gridSide.value + 2);
  const top = gridPadY.value + row * (gridSide.value + 2);
  const palette = paletteFor(cell.slot);
  const hit = cellRowHit(cell);
  let opacity = 1;
  if (rowFocus.value) {
    opacity = hit ? 1 : 0.9;
  } else if (focusSlot.value !== 0) {
    if (cell.slot === 0) opacity = focusSlot.value === -1 ? 1 : 0.35;
    else if (cell.slot === focusSlot.value && (focusItem.value < 0 || cell.item === focusItem.value)) opacity = 1;
    else opacity = 0.35;
  }
  return {
    left: `${left}px`,
    top: `${top}px`,
    width: `${gridSide.value}px`,
    height: `${gridSide.value}px`,
    background: cell.slot === 0 ? 'var(--mz-selected, #edeae6)' : palette.ink,
    opacity: String(opacity),
    boxShadow: hit ? `0 0 0 2px ${palette.ink}` : '',
    zIndex: hit ? '3' : ''
  };
};

const handleVisibleChange = (value: boolean): void => {
  emit('update:open', value);
};

onMounted(() => {
  window.addEventListener('resize', onViewportResize);
});

// The dialog body is re-created on each open, so observe the map once its
// ref binds again (lazy mount); the reported client width drives tile layout.
watch(mapRef, (el) => {
  mapObserver?.disconnect();
  if (!el) return;
  mapObserver = new ResizeObserver(() => {
    if (el.clientWidth > 0) mapWidth.value = el.clientWidth;
  });
  mapObserver.observe(el);
  if (el.clientWidth > 0) mapWidth.value = el.clientWidth;
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
      mapMode.value = true;
      void load();
    } else {
      loadController?.abort();
      textController?.abort();
      detail.value = null;
      record.value = null;
      rowFocus.value = null;
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

.cx-toggle {
  display: flex;
  gap: 2px;
  width: 56px;
  height: 24px;
  padding: 3px 2px;
  border: 1px solid var(--mz-border, #e8e6e3);
  border-radius: 7px;
  background: var(--mz-hover, #f1efec);
  box-sizing: border-box;
}

.cx-toggle-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 16px;
  padding: 0;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: var(--mz-text-muted, #8a8f99);
  cursor: pointer;
}

.cx-toggle-btn svg {
  width: 14px;
  height: 14px;
}

.cx-toggle-btn.is-active {
  background: var(--mz-surface, #ffffff);
  border: 1px solid var(--mz-border, #e8e6e3);
  color: var(--mz-text, #1f2329);
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
  flex: 0 0 auto;
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
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11px;
  font-weight: 600;
}

.cx-tile-value {
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 10px;
  font-weight: 400;
  font-variant-numeric: tabular-nums;
}

.cx-cell {
  position: absolute;
  border: 0;
  border-radius: 2px;
  padding: 0;
  cursor: pointer;
}

.cx-divider {
  height: 1px;
  background: var(--mz-border, #e8e6e3);
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
  background-color: var(--mz-surface, #ffffff);
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
