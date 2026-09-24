<template>
  <span class="reasoning-effort-icon" :style="{ '--reasoning-empty': `${100 - fillPercent}%` }" aria-hidden="true">
    <i class="fa-solid fa-brain reasoning-effort-icon-base"></i>
    <i class="fa-solid fa-brain reasoning-effort-icon-liquid"></i>
  </span>
</template>

<script setup lang="ts">
import { computed } from 'vue';

type ReasoningEffort = 'default' | 'none' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh';
const props = defineProps<{ effort: ReasoningEffort }>();
// The model default is unspecified; do not imply a known reasoning budget.
const levels: Record<ReasoningEffort, number> = {
  default: 0, none: 0, minimal: 15, low: 30, medium: 50, high: 75, xhigh: 100
};
const fillPercent = computed(() => levels[props.effort] ?? 0);
</script>

<style scoped>
.reasoning-effort-icon {
  position: relative;
  display: inline-block;
  flex-shrink: 0;
  width: 18px;
  height: 18px;
  font-size: 16px;
  line-height: 18px;
}

.reasoning-effort-icon > i {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  line-height: inherit;
}

.reasoning-effort-icon-base {
  color: var(--messenger-polish-subtle-muted, #8a919e);
}

.reasoning-effort-icon-liquid {
  /* Crop the existing glyph from the bottom, keeping its silhouette intact. */
  color: var(--el-color-warning, #e6a23c);
  background: linear-gradient(180deg, #ffb35b, #f58a24);
  background-clip: text;
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  clip-path: inset(var(--reasoning-empty) 0 0);
  transition: clip-path 220ms ease;
}

@media (prefers-reduced-motion: reduce) {
  .reasoning-effort-icon-liquid { transition: none; }
}
</style>
