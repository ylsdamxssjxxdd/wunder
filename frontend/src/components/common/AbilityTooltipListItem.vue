<template>
  <div class="ability-tooltip-list-item" :class="toneClass">
    <AbilityIconBadge
      :name="name"
      :description="description"
      :kind="kind"
      :group="group"
      :source="source"
      size="xs"
    />
    <div class="ability-tooltip-list-item__copy">
      <div class="ability-tooltip-list-item__head">
        <div class="ability-tooltip-list-item__name">{{ name }}</div>
        <span v-if="chip" class="ability-tooltip-list-item__chip">{{ chip }}</span>
      </div>
      <div
        class="ability-tooltip-list-item__desc"
        :class="{ 'is-empty': !descriptionText }"
      >
        {{ descriptionText || emptyText }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import AbilityIconBadge from './AbilityIconBadge.vue';
import { resolveAbilityVisual } from '@/utils/abilityVisuals';

const props = withDefaults(
  defineProps<{
    name?: string;
    description?: string;
    kind?: string;
    group?: string;
    source?: string;
    chip?: string;
    emptyText?: string;
  }>(),
  {
    name: '',
    description: '',
    kind: 'tool',
    group: '',
    source: '',
    chip: '',
    emptyText: ''
  }
);

const descriptionText = computed(() => String(props.description || '').trim());

const toneClass = computed(() => {
  const tone = resolveAbilityVisual({
    name: props.name,
    description: props.description,
    kind: props.kind,
    group: props.group,
    source: props.source
  }).tone;
  return `ability-tooltip-list-item--${tone}`;
});
</script>

<style scoped>
.ability-tooltip-list-item {
  min-width: 0;
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 10px 12px;
  border-radius: 12px;
  border: 1px solid rgba(148, 163, 184, 0.24);
  background: rgba(15, 23, 42, 0.04);
}

.ability-tooltip-list-item__copy {
  min-width: 0;
  flex: 1 1 auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.ability-tooltip-list-item__head {
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 8px;
}

.ability-tooltip-list-item__name {
  min-width: 0;
  flex: 1 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--app-text-color, var(--chat-text, #0f172a));
  font-size: 13px;
  font-weight: 700;
  line-height: 1.3;
}

.ability-tooltip-list-item__chip {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  min-height: 20px;
  padding: 0 8px;
  border-radius: 999px;
  border: 1px solid rgba(148, 163, 184, 0.24);
  background: rgba(148, 163, 184, 0.08);
  color: #475569;
  font-size: 10px;
  font-weight: 700;
  line-height: 1;
}

.ability-tooltip-list-item__desc {
  color: var(--app-text-color-secondary, var(--chat-muted, #64748b));
  font-size: 11px;
  line-height: 1.5;
  word-break: break-word;
  overflow-wrap: anywhere;
}

.ability-tooltip-list-item__desc.is-empty {
  color: rgba(100, 116, 139, 0.82);
}

.ability-tooltip-list-item--skill {
  border-color: rgba(245, 158, 11, 0.24);
  background: rgba(245, 158, 11, 0.08);
}

.ability-tooltip-list-item--mcp {
  border-color: rgba(14, 165, 233, 0.24);
  background: rgba(14, 165, 233, 0.08);
}

.ability-tooltip-list-item--knowledge {
  border-color: rgba(16, 185, 129, 0.24);
  background: rgba(16, 185, 129, 0.08);
}

.ability-tooltip-list-item--shared {
  border-color: rgba(139, 92, 246, 0.24);
  background: rgba(139, 92, 246, 0.08);
}

.ability-tooltip-list-item--automation {
  border-color: rgba(99, 102, 241, 0.24);
  background: rgba(99, 102, 241, 0.08);
}

.ability-tooltip-list-item--search {
  border-color: rgba(34, 197, 94, 0.24);
  background: rgba(34, 197, 94, 0.08);
}

.ability-tooltip-list-item--file {
  border-color: rgba(59, 130, 246, 0.24);
  background: rgba(59, 130, 246, 0.08);
}

.ability-tooltip-list-item--terminal {
  border-color: rgba(51, 65, 85, 0.24);
  background: rgba(51, 65, 85, 0.08);
}

:global(:root[data-user-accent='tech-blue'] .ability-tooltip-list-item) {
  background: rgba(15, 23, 42, 0.5);
  border-color: rgba(148, 163, 184, 0.22);
}

:global(:root[data-user-accent='tech-blue'] .ability-tooltip-list-item__name) {
  color: #f8fafc;
}

:global(:root[data-user-accent='tech-blue'] .ability-tooltip-list-item__chip) {
  color: #cbd5e1;
  background: rgba(148, 163, 184, 0.12);
}

:global(:root[data-user-accent='tech-blue'] .ability-tooltip-list-item__desc) {
  color: #cbd5e1;
}

:global(:root[data-user-accent='tech-blue'] .ability-tooltip-list-item__desc.is-empty) {
  color: rgba(203, 213, 225, 0.78);
}
</style>
