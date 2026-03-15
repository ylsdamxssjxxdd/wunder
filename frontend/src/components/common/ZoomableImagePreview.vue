<template>
  <div class="zoomable-image-preview">
    <div v-if="imageUrl" class="zoomable-image-toolbar">
      <button
        class="zoomable-image-btn"
        type="button"
        :title="t('common.zoomOut')"
        :aria-label="t('common.zoomOut')"
        :disabled="!canZoomOut"
        @click="zoomOut"
      >
        <i class="fa-solid fa-magnifying-glass-minus" aria-hidden="true"></i>
      </button>
      <button
        class="zoomable-image-btn"
        type="button"
        :title="t('common.fit')"
        :aria-label="t('common.fit')"
        @click="fitToView"
      >
        <i class="fa-solid fa-maximize" aria-hidden="true"></i>
      </button>
      <button
        class="zoomable-image-btn"
        type="button"
        :title="t('common.reset')"
        :aria-label="t('common.reset')"
        @click="resetZoom"
      >
        <i class="fa-solid fa-rotate-left" aria-hidden="true"></i>
      </button>
      <span class="zoomable-image-scale" aria-live="polite">{{ scaleLabel }}</span>
      <button
        class="zoomable-image-btn"
        type="button"
        :title="t('common.zoomIn')"
        :aria-label="t('common.zoomIn')"
        :disabled="!canZoomIn"
        @click="zoomIn"
      >
        <i class="fa-solid fa-magnifying-glass-plus" aria-hidden="true"></i>
      </button>
    </div>
    <div
      ref="stageRef"
      class="zoomable-image-stage"
      :class="{ 'is-pannable': canPan, 'is-dragging': isDragging }"
      @pointerdown="handlePointerDown"
      @pointermove="handlePointerMove"
      @pointerup="stopDragging"
      @pointercancel="stopDragging"
      @wheel="handleWheel"
      @dblclick="handleDoubleClick"
    >
      <div v-if="imageUrl" class="zoomable-image-hint">
        {{ t('chat.imagePreviewHint') }}
      </div>
      <img
        v-if="imageUrl"
        :src="imageUrl"
        :alt="alt"
        class="zoomable-image"
        :style="imageStyle"
        draggable="false"
        @dragstart.prevent
        @load="handleImageLoad"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';

import { useI18n } from '@/i18n';

const WHEEL_ZOOM_STEP = 0.1;
const DOUBLE_CLICK_TOGGLE_THRESHOLD = 0.05;

const props = withDefaults(
  defineProps<{
    imageUrl: string;
    alt?: string;
    active?: boolean;
    minScale?: number;
    maxScale?: number;
    step?: number;
  }>(),
  {
    alt: '',
    active: true,
    minScale: 0.25,
    maxScale: 3,
    step: 0.25
  }
);

const { t } = useI18n();

const stageRef = ref<HTMLDivElement | null>(null);
const naturalWidth = ref(0);
const naturalHeight = ref(0);
const stageWidth = ref(0);
const stageHeight = ref(0);
const scale = ref(1);
const isDragging = ref(false);
const dragPointerId = ref<number | null>(null);
const dragStartX = ref(0);
const dragStartY = ref(0);
const dragStartScrollLeft = ref(0);
const dragStartScrollTop = ref(0);

let stageResizeObserver: ResizeObserver | null = null;

const renderedWidth = computed(() => naturalWidth.value * scale.value);
const renderedHeight = computed(() => naturalHeight.value * scale.value);
const canZoomOut = computed(() => scale.value > props.minScale + 0.001);
const canZoomIn = computed(() => scale.value < props.maxScale - 0.001);
const canPan = computed(
  () => renderedWidth.value > stageWidth.value + 1 || renderedHeight.value > stageHeight.value + 1
);
const scaleLabel = computed(() => `${Math.round(scale.value * 100)}%`);
const fitScale = computed(() => {
  if (!naturalWidth.value || !naturalHeight.value || !stageWidth.value || !stageHeight.value) {
    return 1;
  }
  const availableWidth = Math.max(1, stageWidth.value - 24);
  const availableHeight = Math.max(1, stageHeight.value - 24);
  return clampScale(Math.min(1, availableWidth / naturalWidth.value, availableHeight / naturalHeight.value));
});
const imageStyle = computed(() => {
  if (!naturalWidth.value) {
    return { maxWidth: '100%' };
  }
  return {
    width: `${Math.max(1, Math.round(renderedWidth.value))}px`,
    maxWidth: 'none'
  };
});

function clampScale(value: number) {
  return Math.min(props.maxScale, Math.max(props.minScale, value));
}

function updateStageMetrics() {
  const stage = stageRef.value;
  stageWidth.value = stage?.clientWidth || 0;
  stageHeight.value = stage?.clientHeight || 0;
}

function bindStageObserver(target: HTMLDivElement | null) {
  if (stageResizeObserver) {
    stageResizeObserver.disconnect();
    stageResizeObserver = null;
  }
  if (target && typeof ResizeObserver !== 'undefined') {
    stageResizeObserver = new ResizeObserver(() => {
      updateStageMetrics();
    });
    stageResizeObserver.observe(target);
  }
  updateStageMetrics();
}

function scheduleAfterLayout(callback: () => void) {
  void nextTick(() => {
    window.requestAnimationFrame(() => {
      updateStageMetrics();
      callback();
    });
  });
}

function setScale(value: number) {
  scale.value = Math.round(clampScale(value) * 100) / 100;
}

function zoomAroundPoint(nextScale: number, clientX?: number, clientY?: number) {
  const stage = stageRef.value;
  if (!stage || !naturalWidth.value || !naturalHeight.value) {
    setScale(nextScale);
    return;
  }
  const previousScale = scale.value || 1;
  const rect = stage.getBoundingClientRect();
  const localX = typeof clientX === 'number'
    ? Math.min(Math.max(0, clientX - rect.left), stage.clientWidth)
    : stage.clientWidth / 2;
  const localY = typeof clientY === 'number'
    ? Math.min(Math.max(0, clientY - rect.top), stage.clientHeight)
    : stage.clientHeight / 2;
  const anchorX = stage.scrollLeft + localX;
  const anchorY = stage.scrollTop + localY;
  setScale(nextScale);
  // Keep the content point under the cursor stable during zoom transitions.
  scheduleAfterLayout(() => {
    const ratio = scale.value / previousScale;
    stage.scrollLeft = Math.max(0, anchorX * ratio - localX);
    stage.scrollTop = Math.max(0, anchorY * ratio - localY);
  });
}

function resetZoom() {
  zoomAroundPoint(1);
}

function zoomOut() {
  zoomAroundPoint(scale.value - props.step);
}

function zoomIn() {
  zoomAroundPoint(scale.value + props.step);
}

function fitToView() {
  const stage = stageRef.value;
  setScale(fitScale.value);
  if (!stage) {
    return;
  }
  scheduleAfterLayout(() => {
    stage.scrollLeft = 0;
    stage.scrollTop = 0;
  });
}

function handleImageLoad(event: Event) {
  const target = event.target as HTMLImageElement | null;
  if (!target) return;
  naturalWidth.value = target.naturalWidth || target.width || 0;
  naturalHeight.value = target.naturalHeight || target.height || 0;
  fitToView();
}

function stopDragging(event?: PointerEvent) {
  const stage = stageRef.value;
  if (event && dragPointerId.value !== null && event.pointerId != dragPointerId.value) {
    return;
  }
  if (stage && dragPointerId.value !== null && stage.hasPointerCapture(dragPointerId.value)) {
    stage.releasePointerCapture(dragPointerId.value);
  }
  dragPointerId.value = null;
  isDragging.value = false;
}

function handlePointerDown(event: PointerEvent) {
  const stage = stageRef.value;
  if (!stage || !canPan.value || event.button !== 0) return;
  if (!(event.target instanceof HTMLImageElement)) return;
  dragPointerId.value = event.pointerId;
  dragStartX.value = event.clientX;
  dragStartY.value = event.clientY;
  dragStartScrollLeft.value = stage.scrollLeft;
  dragStartScrollTop.value = stage.scrollTop;
  isDragging.value = true;
  stage.setPointerCapture(event.pointerId);
  event.preventDefault();
}

function handlePointerMove(event: PointerEvent) {
  const stage = stageRef.value;
  if (!stage || !isDragging.value || dragPointerId.value !== event.pointerId) return;
  stage.scrollLeft = Math.max(0, dragStartScrollLeft.value - (event.clientX - dragStartX.value));
  stage.scrollTop = Math.max(0, dragStartScrollTop.value - (event.clientY - dragStartY.value));
}

function handleWheel(event: WheelEvent) {
  if (!event.ctrlKey && !event.metaKey) {
    return;
  }
  event.preventDefault();
  const direction = event.deltaY < 0 ? WHEEL_ZOOM_STEP : -WHEEL_ZOOM_STEP;
  zoomAroundPoint(scale.value + direction, event.clientX, event.clientY);
}

function handleDoubleClick(event: MouseEvent) {
  if (!(event.target instanceof HTMLImageElement)) {
    return;
  }
  const targetScale = Math.abs(scale.value - fitScale.value) <= DOUBLE_CLICK_TOGGLE_THRESHOLD ? 1 : fitScale.value;
  zoomAroundPoint(targetScale, event.clientX, event.clientY);
}

watch(
  stageRef,
  (nextStage) => {
    bindStageObserver(nextStage);
  },
  { flush: 'post' }
);

watch(
  () => props.imageUrl,
  (nextUrl, previousUrl) => {
    if (nextUrl === previousUrl) return;
    naturalWidth.value = 0;
    naturalHeight.value = 0;
    setScale(1);
    stopDragging();
  }
);

watch(
  () => props.active,
  (nextActive, previousActive) => {
    if (!nextActive || nextActive === previousActive) return;
    setScale(1);
    stopDragging();
    if (naturalWidth.value > 0) {
      fitToView();
    }
  }
);

onBeforeUnmount(() => {
  stopDragging();
  if (stageResizeObserver) {
    stageResizeObserver.disconnect();
    stageResizeObserver = null;
  }
});
</script>

<style scoped>
.zoomable-image-preview {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.zoomable-image-toolbar {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  flex-wrap: wrap;
}

.zoomable-image-btn {
  width: 32px;
  height: 32px;
  border-radius: 8px;
  border: 1px solid rgba(77, 216, 255, 0.2);
  background: rgba(15, 23, 42, 0.68);
  color: var(--chat-text);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: border-color 0.2s ease, background 0.2s ease, transform 0.2s ease;
}

.zoomable-image-btn:hover:not(:disabled),
.zoomable-image-btn:focus-visible:not(:disabled) {
  border-color: rgba(77, 216, 255, 0.4);
  background: rgba(26, 39, 68, 0.9);
  transform: translateY(-1px);
}

.zoomable-image-btn:disabled {
  cursor: not-allowed;
  opacity: 0.45;
}

.zoomable-image-scale {
  min-width: 56px;
  text-align: center;
  font-size: 12px;
  color: var(--chat-muted);
}

.zoomable-image-stage {
  position: relative;
  min-height: 240px;
  max-height: 60vh;
  overflow: auto;
}

.zoomable-image-stage.is-pannable {
  cursor: grab;
}

.zoomable-image-stage.is-dragging {
  cursor: grabbing;
  user-select: none;
}

.zoomable-image-hint {
  position: sticky;
  top: 12px;
  left: calc(100% - 12px);
  z-index: 1;
  margin-left: auto;
  margin-right: 12px;
  max-width: min(360px, calc(100% - 24px));
  padding: 8px 10px;
  border: 1px solid rgba(77, 216, 255, 0.2);
  border-radius: 10px;
  background: rgba(8, 13, 24, 0.78);
  color: var(--chat-muted);
  font-size: 12px;
  line-height: 1.45;
  pointer-events: none;
}

.zoomable-image {
  display: block;
  height: auto;
  margin: 0 auto;
}
</style>
