<template>
  <span
    class="context-usage-icon"
    :style="{ '--context-empty': `${100 - fillPercent}%`, '--context-liquid': liquidColor }"
    role="img"
    :title="title || undefined"
    :aria-label="title || undefined"
  >
    <i class="fa-solid fa-brain context-usage-icon-base" aria-hidden="true"></i>
    <i class="fa-solid fa-brain context-usage-icon-liquid" aria-hidden="true"></i>
  </span>
</template>

<script setup lang="ts">
import { computed } from 'vue';

// Liquid fill tracks context window occupancy: null/0 stays an empty gray glyph,
// and the liquid rises toward full as the context window fills up.
const props = defineProps<{
  ratio: number | null;
  title?: string;
  compacted?: boolean;
}>();

const clampRatio = (value: number): number => Math.min(1, Math.max(0, value));

const fillPercent = computed(() => {
  const ratio = Number(props.ratio);
  if (!Number.isFinite(ratio) || ratio <= 0) return 0;
  return Math.round(clampRatio(ratio) * 100);
});

// crates/wunder-desktop/frontend/ui/composer.slint is the source of truth for
// the liquid tone: flat orange in the safe range, deeper orange near the limit,
// red at the hard limit. Keep these values and thresholds in sync with the
// desktop gauge so the same occupancy reads as the same colour on both clients.
const LIQUID_SAFE_COLOR = '#f97316';
const LIQUID_WARNING_COLOR = '#ea580c';
const LIQUID_DANGER_COLOR = '#dc2626';
const LIQUID_WARNING_RATIO = 0.82;
const LIQUID_DANGER_RATIO = 0.92;

// A compacted thread keeps the danger tone at any occupancy: red then means
// "this history is a summary", not "you are about to run out of room".
const liquidColor = computed(() => {
  if (props.compacted) return LIQUID_DANGER_COLOR;
  const ratio = Number(props.ratio);
  if (!Number.isFinite(ratio)) return LIQUID_SAFE_COLOR;
  if (ratio >= LIQUID_DANGER_RATIO) return LIQUID_DANGER_COLOR;
  if (ratio >= LIQUID_WARNING_RATIO) return LIQUID_WARNING_COLOR;
  return LIQUID_SAFE_COLOR;
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
  background: var(--context-liquid, #f97316);
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
