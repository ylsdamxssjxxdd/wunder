<template>
  <div class="companion-sprite" :style="rootStyle" aria-hidden="true">
    <div class="companion-sprite__sheet" :style="sheetStyle"></div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';

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
    paused?: boolean;
  }>(),
  {
    state: 'idle',
    scale: 1,
    paused: false
  }
);

const frameIndex = ref(0);
let animationTimer: number | null = null;

const normalizedScale = computed(() => Math.min(1.8, Math.max(0.5, Number(props.scale) || 1)));
const stateConfig = computed(() => STATE_CONFIG[props.state] || STATE_CONFIG.idle);

const rootStyle = computed(() => ({
  width: `${FRAME_WIDTH * normalizedScale.value}px`,
  height: `${FRAME_HEIGHT * normalizedScale.value}px`
}));

const sheetStyle = computed(() => ({
  width: `${FRAME_WIDTH}px`,
  height: `${FRAME_HEIGHT}px`,
  backgroundImage: `url("${props.source}")`,
  backgroundPosition: `-${frameIndex.value * FRAME_WIDTH}px -${stateConfig.value.row * FRAME_HEIGHT}px`,
  transform: `scale(${normalizedScale.value})`
}));

const stopAnimation = () => {
  if (animationTimer !== null && typeof window !== 'undefined') {
    window.clearInterval(animationTimer);
  }
  animationTimer = null;
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

onBeforeUnmount(() => {
  stopAnimation();
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
  left: 0;
  top: 0;
  background-repeat: no-repeat;
  transform-origin: left top;
  will-change: background-position;
}
</style>
