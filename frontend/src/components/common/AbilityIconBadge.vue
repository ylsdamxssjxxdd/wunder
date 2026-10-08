<template>
  <span class="ability-icon-badge" :class="[sizeClass, toneClass]" aria-hidden="true">
    <i class="fa-solid ability-icon-badge__icon" :class="visual.icon"></i>
  </span>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { resolveAbilityVisual } from '@/utils/abilityVisuals';

const props = withDefaults(
  defineProps<{
    name?: string;
    description?: string;
    hint?: string;
    kind?: string;
    group?: string;
    source?: string;
    size?: 'xs' | 'sm' | 'md';
  }>(),
  {
    name: '',
    description: '',
    hint: '',
    kind: 'tool',
    group: '',
    source: '',
    size: 'md'
  }
);

const visual = computed(() =>
  resolveAbilityVisual({
    name: props.name,
    description: props.description,
    hint: props.hint,
    kind: props.kind,
    group: props.group,
    source: props.source
  })
);

const sizeClass = computed(() => `ability-icon-badge--${props.size}`);
const toneClass = computed(() => `ability-icon-badge--${visual.value.tone}`);
</script>

<style scoped>
.ability-icon-badge {
  --ability-icon-size: 36px;
  --ability-icon-radius: 12px;
  position: relative;
  width: var(--ability-icon-size);
  height: var(--ability-icon-size);
  min-width: var(--ability-icon-size);
  border-radius: var(--ability-icon-radius);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  border: 1px solid rgba(148, 163, 184, 0.22);
  background: #ffffff;
  color: #334155;
  flex-shrink: 0;
}

.ability-icon-badge__icon {
  position: relative;
  z-index: 1;
  font-size: calc(var(--ability-icon-size) * 0.42);
  line-height: 1;
}

.ability-icon-badge__icon.fa-bee {
  width: 1em;
  height: 1em;
  transform: scale(1.7);
  transform-origin: center;
}

.ability-icon-badge__icon.fa-bee::before {
  content: '';
}

.ability-icon-badge__icon.fa-bee::after {
  content: '';
  position: absolute;
  inset: 0;
  background-color: currentColor;
  -webkit-mask: url('../../assets/fa-bee.svg') center / contain no-repeat;
  mask: url('../../assets/fa-bee.svg') center / contain no-repeat;
}

.ability-icon-badge--xs {
  --ability-icon-size: 24px;
  --ability-icon-radius: 8px;
}

.ability-icon-badge--sm {
  --ability-icon-size: 30px;
  --ability-icon-radius: 10px;
}

.ability-icon-badge--md {
  --ability-icon-size: 36px;
  --ability-icon-radius: 12px;
}

.ability-icon-badge--skill {
  border-color: rgba(245, 158, 11, 0.24);
  background: rgba(245, 158, 11, 0.12);
  color: #b45309;
}

.ability-icon-badge--mcp {
  border-color: rgba(14, 165, 233, 0.24);
  background: rgba(14, 165, 233, 0.12);
  color: #0369a1;
}

.ability-icon-badge--knowledge {
  border-color: rgba(16, 185, 129, 0.24);
  background: rgba(16, 185, 129, 0.12);
  color: #047857;
}

.ability-icon-badge--shared {
  border-color: rgba(139, 92, 246, 0.24);
  background: rgba(139, 92, 246, 0.12);
  color: #6d28d9;
}

.ability-icon-badge--automation {
  border-color: rgba(99, 102, 241, 0.24);
  background: rgba(99, 102, 241, 0.12);
  color: #4338ca;
}

.ability-icon-badge--search {
  border-color: rgba(34, 197, 94, 0.24);
  background: rgba(34, 197, 94, 0.12);
  color: #15803d;
}

.ability-icon-badge--file {
  border-color: rgba(59, 130, 246, 0.24);
  background: rgba(59, 130, 246, 0.12);
  color: #1d4ed8;
}

.ability-icon-badge--terminal {
  border-color: rgba(51, 65, 85, 0.26);
  background: rgba(51, 65, 85, 0.12);
  color: #0f172a;
}

:global(:root[data-user-accent='tech-blue'] .ability-icon-badge) {
  background: rgba(15, 23, 42, 0.9);
  border-color: rgba(148, 163, 184, 0.22);
  color: #e2e8f0;
}

:global(:root[data-user-accent='tech-blue'] .ability-icon-badge--skill) {
  background: rgba(245, 158, 11, 0.18);
  color: #fbbf24;
}

:global(:root[data-user-accent='tech-blue'] .ability-icon-badge--mcp) {
  background: rgba(14, 165, 233, 0.18);
  color: #7dd3fc;
}

:global(:root[data-user-accent='tech-blue'] .ability-icon-badge--knowledge) {
  background: rgba(16, 185, 129, 0.18);
  color: #6ee7b7;
}

:global(:root[data-user-accent='tech-blue'] .ability-icon-badge--shared) {
  background: rgba(139, 92, 246, 0.18);
  color: #c4b5fd;
}

:global(:root[data-user-accent='tech-blue'] .ability-icon-badge--automation) {
  background: rgba(99, 102, 241, 0.18);
  color: #a5b4fc;
}

:global(:root[data-user-accent='tech-blue'] .ability-icon-badge--search) {
  background: rgba(34, 197, 94, 0.18);
  color: #86efac;
}

:global(:root[data-user-accent='tech-blue'] .ability-icon-badge--file) {
  background: rgba(59, 130, 246, 0.18);
  color: #93c5fd;
}

:global(:root[data-user-accent='tech-blue'] .ability-icon-badge--terminal) {
  background: rgba(51, 65, 85, 0.9);
  color: #e2e8f0;
}
</style>
