<template>
  <div class="ability-tooltip-card" :class="toneClass">
    <div class="ability-tooltip-card__hero">
      <AbilityIconBadge
        :name="name"
        :description="description"
        :hint="hint"
        :kind="kind"
        :group="group"
        :source="source"
        size="md"
      />
      <div class="ability-tooltip-card__copy">
        <div class="ability-tooltip-card__name">{{ name }}</div>
        <div class="ability-tooltip-card__summary">{{ primaryText }}</div>
      </div>
    </div>
    <div v-if="chips.length" class="ability-tooltip-card__chips">
      <span v-for="chip in chips" :key="chip" class="ability-tooltip-card__chip">{{ chip }}</span>
    </div>
    <div v-if="secondaryText" class="ability-tooltip-card__detail">{{ secondaryText }}</div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { useI18n } from '@/i18n';
import { resolveAbilityPitchKey, resolveAbilitySummary, resolveAbilityVisual } from '@/utils/abilityVisuals';
import AbilityIconBadge from './AbilityIconBadge.vue';

const props = withDefaults(
  defineProps<{
    name?: string;
    description?: string;
    hint?: string;
    kind?: string;
    group?: string;
    source?: string;
    showDetail?: boolean;
    chips?: string[];
  }>(),
  {
    name: '',
    description: '',
    hint: '',
    kind: 'tool',
    group: '',
    source: '',
    showDetail: true,
    chips: () => []
  }
);

const { t } = useI18n();

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

const toneClass = computed(() => `ability-tooltip-card--${visual.value.tone}`);

const primaryText = computed(() => {
  const summary = resolveAbilitySummary(props.description, '');
  if (summary) {
    return summary;
  }
  return t(resolveAbilityPitchKey(props));
});

const secondaryText = computed(() => {
  if (!props.showDetail) {
    return '';
  }
  const detail = resolveAbilitySummary(props.hint, '');
  if (!detail || detail === primaryText.value) {
    return '';
  }
  return detail;
});

const chips = computed(() => {
  const seen = new Set<string>();
  return props.chips
    .map((item) => String(item || '').trim())
    .filter((item) => {
      if (!item || seen.has(item)) {
        return false;
      }
      seen.add(item);
      return true;
    });
});
</script>

<style scoped>
.ability-tooltip-card {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: min(300px, calc(100vw - 48px));
  max-width: min(360px, calc(100vw - 36px));
  padding: 14px;
  border-radius: 16px;
  border: 1px solid rgba(148, 163, 184, 0.22);
  color: #0f172a;
  background: #ffffff;
  box-sizing: border-box;
}

.ability-tooltip-card__hero {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  min-width: 0;
}

.ability-tooltip-card__copy {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.ability-tooltip-card__name {
  color: #0f172a;
  font-size: 14px;
  font-weight: 700;
  line-height: 1.35;
  word-break: break-word;
  overflow-wrap: anywhere;
}

.ability-tooltip-card__summary {
  color: #475569;
  font-size: 12px;
  line-height: 1.55;
  word-break: break-word;
  overflow-wrap: anywhere;
}

.ability-tooltip-card__chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.ability-tooltip-card__chip {
  display: inline-flex;
  align-items: center;
  min-height: 24px;
  padding: 0 10px;
  border-radius: 999px;
  border: 1px solid rgba(148, 163, 184, 0.24);
  background: rgba(148, 163, 184, 0.08);
  color: #334155;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.01em;
}

.ability-tooltip-card__detail {
  border: 1px solid rgba(148, 163, 184, 0.18);
  border-radius: 12px;
  background: rgba(148, 163, 184, 0.06);
  padding: 10px 12px;
  color: #475569;
  font-size: 12px;
  line-height: 1.55;
  word-break: break-word;
  overflow-wrap: anywhere;
}

.ability-tooltip-card--skill {
  border-color: rgba(245, 158, 11, 0.22);
}

.ability-tooltip-card--mcp {
  border-color: rgba(14, 165, 233, 0.22);
}

.ability-tooltip-card--knowledge {
  border-color: rgba(16, 185, 129, 0.22);
}

.ability-tooltip-card--shared {
  border-color: rgba(139, 92, 246, 0.22);
}

.ability-tooltip-card--automation {
  border-color: rgba(99, 102, 241, 0.22);
}

.ability-tooltip-card--search {
  border-color: rgba(34, 197, 94, 0.22);
}

:global(.ability-card-popper.el-popper),
:global(.ability-card-popper.el-popper.is-dark) {
  background: transparent !important;
  border: 0 !important;
  box-shadow: none !important;
  padding: 0 !important;
  overflow: visible !important;
}

:global(.ability-card-popper .el-popper__arrow) {
  display: none !important;
}

:global(.ability-card-popper::before) {
  content: none !important;
}

:global(:root[data-user-theme='dark'] .ability-tooltip-card) {
  color: #e2e8f0;
  background: #0f172a;
  border-color: rgba(148, 163, 184, 0.24);
}

:global(:root[data-user-theme='dark'] .ability-tooltip-card__name) {
  color: #f8fafc;
}

:global(:root[data-user-theme='dark'] .ability-tooltip-card__summary) {
  color: #cbd5e1;
}

:global(:root[data-user-theme='dark'] .ability-tooltip-card__chip) {
  border-color: rgba(148, 163, 184, 0.26);
  background: rgba(148, 163, 184, 0.14);
  color: #e2e8f0;
}

:global(:root[data-user-theme='dark'] .ability-tooltip-card__detail) {
  border-color: rgba(148, 163, 184, 0.2);
  background: rgba(148, 163, 184, 0.08);
  color: #cbd5e1;
}
</style>
