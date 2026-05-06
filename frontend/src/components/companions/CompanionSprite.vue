<template>
  <div ref="rootRef" class="companion-sprite" :style="rootStyle" aria-hidden="true">
    <div class="companion-sprite__sheet" :style="sheetStyle"></div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import type { CSSProperties } from 'vue';

import type { CompanionSpriteStateId } from '@/stores/companions';

type SpriteStateConfig = {
  row: number;
  frames: number;
  duration: number;
};

const FRAME_WIDTH = 192;
const FRAME_HEIGHT = 208;

const STATE_CONFIG: Record<CompanionSpriteStateId, SpriteStateConfig> = {
  idle: { row: 0, frames: 6, duration: 1100 },
  'running-right': { row: 1, frames: 8, duration: 1060 },
  'running-left': { row: 2, frames: 8, duration: 1060 },
  waving: { row: 3, frames: 4, duration: 700 },
  jumping: { row: 4, frames: 5, duration: 840 },
  failed: { row: 5, frames: 8, duration: 1220 },
  waiting: { row: 6, frames: 6, duration: 1010 },
  running: { row: 7, frames: 6, duration: 820 },
  review: { row: 8, frames: 6, duration: 1030 }
};

const props = withDefaults(
  defineProps<{
    source: string;
    state?: CompanionSpriteStateId;
    scale?: number;
    fit?: boolean;
    paused?: boolean;
  }>(),
  {
    state: 'idle',
    scale: 1,
    fit: false,
    paused: false
  }
);

const frameIndex = ref(0);
const rootRef = ref<HTMLElement | null>(null);
const rootSize = ref({ width: 0, height: 0 });
let animationTimer: number | null = null;
let resizeObserver: ResizeObserver | null = null;
let resizeListenerAttached = false;

const normalizedScale = computed(() => Math.min(1.8, Math.max(0.5, Number(props.scale) || 1)));
const stateConfig = computed(() => STATE_CONFIG[props.state] || STATE_CONFIG.idle);
const fitScale = computed(() => {
  if (!props.fit) {
    return 1;
  }
  const width = rootSize.value.width;
  const height = rootSize.value.height;
  if (!width || !height) {
    return 1;
  }
  return Math.min(width / FRAME_WIDTH, height / FRAME_HEIGHT);
});
const renderedScale = computed(() => normalizedScale.value * (props.fit ? fitScale.value : 1));
const fitReady = computed(() => !props.fit || (rootSize.value.width > 0 && rootSize.value.height > 0));

const rootStyle = computed(() => ({
  width: props.fit ? '100%' : `${FRAME_WIDTH * normalizedScale.value}px`,
  height: props.fit ? '100%' : `${FRAME_HEIGHT * normalizedScale.value}px`
}));

const sheetStyle = computed<CSSProperties>(() => ({
  width: `${FRAME_WIDTH}px`,
  height: `${FRAME_HEIGHT}px`,
  backgroundImage: `url("${props.source}")`,
  backgroundPosition: `-${frameIndex.value * FRAME_WIDTH}px -${stateConfig.value.row * FRAME_HEIGHT}px`,
  left: props.fit ? '50%' : '0',
  top: props.fit ? '50%' : '0',
  transformOrigin: props.fit ? 'center center' : 'left top',
  transform: props.fit
    ? `translate(-50%, -50%) scale(${renderedScale.value})`
    : `scale(${renderedScale.value})`,
  visibility: fitReady.value ? 'visible' : 'hidden'
}));

const updateRootSize = () => {
  const element = rootRef.value;
  if (!element) {
    return;
  }
  const rect = element.getBoundingClientRect();
  rootSize.value = {
    width: Math.max(0, Math.round(rect.width || element.clientWidth || 0)),
    height: Math.max(0, Math.round(rect.height || element.clientHeight || 0))
  };
};

const stopAnimation = () => {
  if (animationTimer !== null && typeof window !== 'undefined') {
    window.clearInterval(animationTimer);
  }
  animationTimer = null;
};

const stopResizeTracking = () => {
  if (resizeObserver) {
    resizeObserver.disconnect();
    resizeObserver = null;
  }
  if (resizeListenerAttached && typeof window !== 'undefined') {
    window.removeEventListener('resize', updateRootSize);
    resizeListenerAttached = false;
  }
};

const startResizeTracking = () => {
  stopResizeTracking();
  if (typeof window === 'undefined') {
    return;
  }
  updateRootSize();
  if (typeof ResizeObserver !== 'undefined' && rootRef.value) {
    resizeObserver = new ResizeObserver(() => {
      updateRootSize();
    });
    resizeObserver.observe(rootRef.value);
    return;
  }
  window.addEventListener('resize', updateRootSize);
  resizeListenerAttached = true;
};

const startAnimation = () => {
  stopAnimation();
  if (props.paused || typeof window === 'undefined') {
    return;
  }
  const config = stateConfig.value;
  const frameMs = Math.max(50, Math.round(config.duration / Math.max(1, config.frames)));
  animationTimer = window.setInterval(() => {
    frameIndex.value = (frameIndex.value + 1) % config.frames;
  }, frameMs);
};

watch(
  () => [props.state, props.source, props.paused] as const,
  () => {
    frameIndex.value = 0;
    startAnimation();
  },
  { immediate: true }
);

watch(
  () => [props.fit, props.scale] as const,
  () => {
    if (props.fit) {
      updateRootSize();
    }
  }
);

onMounted(() => {
  if (props.fit) {
    startResizeTracking();
  }
});

watch(
  () => props.fit,
  (value) => {
    if (value) {
      startResizeTracking();
      return;
    }
    stopResizeTracking();
    rootSize.value = { width: 0, height: 0 };
  },
  { immediate: true }
);

onBeforeUnmount(() => {
  stopAnimation();
  stopResizeTracking();
});
</script>

<style scoped>
.companion-sprite {
  position: relative;
  overflow: hidden;
  flex: 0 0 auto;
}

.companion-sprite__sheet {
  position: absolute;
  background-repeat: no-repeat;
  will-change: background-position;
}
</style>
