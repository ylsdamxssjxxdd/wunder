<template>
  <span
    class="context-usage-icon"
    :style="{ '--context-empty': `${100 - fillPercent}%`, '--context-liquid': liquidGradient }"
    role="img"
    :title="title || undefined"
    :aria-label="title || undefined"
  >
    <i class="fa-solid fa-brain context-usage-icon-base"></i>
    <i class="fa-solid fa-brain context-usage-icon-liquid"></i>
  </span>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { CONTEXT_DANGER_RATIO, CONTEXT_WARNING_RATIO } from '@/components/chat/composerContextUsage';

// Liquid fill tracks context window occupancy: null/0 stays an empty gray glyph,
// and the liquid rises toward full as the context window fills up.
const props = defineProps<{ ratio: number | null; title?: string }>();

const clampRatio = (value: number): number => Math.min(1, Math.max(0, value));

const fillPercent = computed(() => {
  const ratio = Number(props.ratio);
  if (!Number.isFinite(ratio) || ratio <= 0) return 0;
  return Math.round(clampRatio(ratio) * 100);
});

const lerpChannel = (from: number, to: number, progress: number): number =>
  Math.round(from + (to - from) * progress);

// Keep the calm amber look in the safe range, then blend toward red as the
// window approaches the hard limit so the icon itself carries the old
// warning/danger signal from the removed text.
const liquidGradient = computed(() => {
  const ratio = Number(props.ratio);
  if (!Number.isFinite(ratio) || ratio < CONTEXT_WARNING_RATIO) {
    return 'linear-gradient(180deg, #ffb35b, #f58a24)';
  }
  const span = Math.max(0.01, CONTEXT_DANGER_RATIO - CONTEXT_WARNING_RATIO);
  const progress = clampRatio((ratio - CONTEXT_WARNING_RATIO) / span);
  const top = `rgb(${lerpChannel(255, 255, progress)}, ${lerpChannel(179, 138, progress)}, ${lerpChannel(91, 92, progress)})`;
  const bottom = `rgb(${lerpChannel(245, 220, progress)}, ${lerpChannel(138, 38, progress)}, ${lerpChannel(36, 38, progress)})`;
  return `linear-gradient(180deg, ${top}, ${bottom})`;
});
</script>

<style scoped>
.context-usage-icon {
  position: relative;
  display: inline-block;
  flex-shrink: 0;
  width: 18px;
  height: 18px;
  font-size: 16px;
  line-height: 18px;
}

.context-usage-icon > i {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  line-height: inherit;
}

.context-usage-icon-base {
  color: var(--messenger-polish-subtle-muted, #8a919e);
}

.context-usage-icon-liquid {
  /* Crop the existing glyph from the bottom, keeping its silhouette intact. */
  background: var(--context-liquid, linear-gradient(180deg, #ffb35b, #f58a24));
  background-clip: text;
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  clip-path: inset(var(--context-empty, 100%) 0 0);
  transition: clip-path 220ms ease;
}

@media (prefers-reduced-motion: reduce) {
  .context-usage-icon-liquid { transition: none; }
}
</style>
