<template>
  <!--
    覆盖式细滚动条：云端版全局隐藏了原生滚动条，滚动容器用这一层自绘滑块
    提示「可以滚动、滚到哪儿了」。轨道只占贴边 8px 且不拦截指针，只有滑块
    自身可拖动，所以不会挡住内容里的文字选择与点击。
  -->
  <div
    ref="trackRef"
    class="thin-scrollbar"
    :class="[`is-${axis}`, { 'is-scrollable': metrics.scrollable, 'is-active': dragging }]"
    aria-hidden="true"
  >
    <span class="thin-scrollbar-thumb" :style="thumbStyle" @pointerdown="onThumbPointerDown" />
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';

import { overlayScrollbarDragOffset, overlayScrollbarMetrics, type OverlayScrollbarMetrics } from '@/utils/overlayScrollbar';

const props = withDefaults(
  defineProps<{
    /** 目标滚动容器；必须与本组件同处一个 position: relative 的包装层内。 */
    target?: HTMLElement | null;
    axis?: 'x' | 'y';
  }>(),
  { target: null, axis: 'y' }
);

const trackRef = ref<HTMLElement | null>(null);
const metrics = ref<OverlayScrollbarMetrics>({ scrollable: false, thumbSize: 0, thumbOffset: 0 });
const dragging = ref(false);

const vertical = computed(() => props.axis !== 'x');

const thumbStyle = computed(() => {
  const style: Record<string, string> = { transform: '' };
  if (vertical.value) {
    style.height = `${metrics.value.thumbSize}px`;
    style.transform = `translateY(${metrics.value.thumbOffset}px)`;
  } else {
    style.width = `${metrics.value.thumbSize}px`;
    style.transform = `translateX(${metrics.value.thumbOffset}px)`;
  }
  return style;
});

let scroller: HTMLElement | null = null;
let resizeObserver: ResizeObserver | null = null;
let mutationObserver: MutationObserver | null = null;
let measureFrame = 0;

function measure(): void {
  const el = scroller;
  const track = trackRef.value;
  if (!el || !track) return;
  const along = vertical.value;
  metrics.value = overlayScrollbarMetrics({
    viewport: along ? el.clientHeight : el.clientWidth,
    content: along ? el.scrollHeight : el.scrollWidth,
    offset: along ? el.scrollTop : el.scrollLeft,
    track: along ? track.clientHeight : track.clientWidth
  });
}

/** 结构变化（懒加载补记录、流式追加行）合并到一帧里量一次。 */
function scheduleMeasure(): void {
  if (measureFrame) return;
  measureFrame = requestAnimationFrame(() => {
    measureFrame = 0;
    measure();
  });
}

// 滚动位置必须立刻反映到滑块上，晚一帧就能看出滑块「追不上」内容。
function onScroll(): void {
  measure();
}

function detach(): void {
  if (measureFrame) {
    cancelAnimationFrame(measureFrame);
    measureFrame = 0;
  }
  resizeObserver?.disconnect();
  resizeObserver = null;
  mutationObserver?.disconnect();
  mutationObserver = null;
  if (scroller) {
    scroller.removeEventListener('scroll', onScroll);
    scroller.removeEventListener('pointerenter', scheduleMeasure);
  }
  scroller = null;
  // 目标被换掉时先收起旧滑块，避免旧容器的尺寸被继续显示。
  metrics.value = { scrollable: false, thumbSize: 0, thumbOffset: 0 };
}

function attach(el: HTMLElement | null): void {
  detach();
  if (el) {
    scroller = el;
    el.addEventListener('scroll', onScroll, { passive: true });
    // 内容可能在不滚动的情况下变长（懒加载补行、流式追加），指针进入时补量一次。
    el.addEventListener('pointerenter', scheduleMeasure);
    if (typeof ResizeObserver === 'function') {
      resizeObserver = new ResizeObserver(scheduleMeasure);
      resizeObserver.observe(el);
      if (trackRef.value) resizeObserver.observe(trackRef.value);
    }
    if (typeof MutationObserver === 'function') {
      // 只监听结构变化：流式文本走 characterData，不会把这里刷成高频回调。
      mutationObserver = new MutationObserver(scheduleMeasure);
      mutationObserver.observe(el, { childList: true, subtree: true });
    }
  }
  scheduleMeasure();
}

watch(
  () => props.target,
  (el) => {
    void nextTick(() => attach(el ?? null));
  }
);

onMounted(() => {
  attach(props.target ?? null);
});

onBeforeUnmount(() => {
  detach();
  stopDrag();
});

let dragOrigin: { pointer: number; offset: number } | null = null;

function onThumbPointerDown(event: PointerEvent): void {
  const el = scroller;
  if (!el || event.button !== 0) return;
  event.preventDefault();
  event.stopPropagation();
  dragging.value = true;
  dragOrigin = {
    pointer: vertical.value ? event.clientY : event.clientX,
    offset: vertical.value ? el.scrollTop : el.scrollLeft
  };
  window.addEventListener('pointermove', onDragMove);
  window.addEventListener('pointerup', onDragEnd);
  window.addEventListener('pointercancel', onDragEnd);
}

function onDragMove(event: PointerEvent): void {
  const el = scroller;
  const track = trackRef.value;
  if (!el || !track || !dragOrigin) return;
  const along = vertical.value;
  const delta = (along ? event.clientY : event.clientX) - dragOrigin.pointer;
  const shift = overlayScrollbarDragOffset({
    delta,
    track: along ? track.clientHeight : track.clientWidth,
    thumbSize: metrics.value.thumbSize,
    viewport: along ? el.clientHeight : el.clientWidth,
    content: along ? el.scrollHeight : el.scrollWidth
  });
  if (along) el.scrollTop = dragOrigin.offset + shift;
  else el.scrollLeft = dragOrigin.offset + shift;
  measure();
}

function onDragEnd(): void {
  stopDrag();
  measure();
}

function stopDrag(): void {
  dragOrigin = null;
  dragging.value = false;
  window.removeEventListener('pointermove', onDragMove);
  window.removeEventListener('pointerup', onDragEnd);
  window.removeEventListener('pointercancel', onDragEnd);
}
</script>

<style scoped>
.thin-scrollbar {
  position: absolute;
  z-index: 3;
  pointer-events: none;
  opacity: 0;
  transition: opacity 0.16s ease;
}

.thin-scrollbar.is-y {
  top: 0;
  right: 0;
  bottom: 0;
  width: 8px;
}

.thin-scrollbar.is-x {
  left: 0;
  right: 0;
  bottom: 0;
  height: 8px;
}

/* 可滚动时常显：细条本身就是「这里还能滚」的提示，不靠悬停才发现。 */
.thin-scrollbar.is-scrollable {
  opacity: 1;
}

.thin-scrollbar-thumb {
  position: absolute;
  border-radius: 3px;
  background: var(--thin-scrollbar-thumb, rgba(31, 35, 41, 0.24));
  /* 细条不可见时不许吃掉指针事件，只有真正可滚动时才可抓。 */
  pointer-events: none;
  cursor: grab;
  transition: background-color 0.16s ease;
}

.thin-scrollbar.is-scrollable .thin-scrollbar-thumb {
  pointer-events: auto;
}

.thin-scrollbar-thumb:hover,
.thin-scrollbar.is-active .thin-scrollbar-thumb {
  background: var(--thin-scrollbar-thumb-hover, rgba(31, 35, 41, 0.44));
}

.thin-scrollbar.is-active .thin-scrollbar-thumb {
  cursor: grabbing;
}

.thin-scrollbar.is-y .thin-scrollbar-thumb {
  top: 0;
  right: 2px;
  width: 4px;
  min-height: 24px;
}

.thin-scrollbar.is-x .thin-scrollbar-thumb {
  left: 0;
  bottom: 2px;
  height: 4px;
  min-width: 24px;
}

/* 4px 的细条不好抓，左右各放 3px 透明命中区。 */
.thin-scrollbar.is-y .thin-scrollbar-thumb::before {
  content: '';
  position: absolute;
  inset: 0 -3px;
}

.thin-scrollbar.is-x .thin-scrollbar-thumb::before {
  content: '';
  position: absolute;
  inset: -3px 0;
}
</style>
