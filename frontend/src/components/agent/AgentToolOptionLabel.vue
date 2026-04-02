<template>
  <div class="agent-tool-option-label" :title="titleText">
    <AbilityIconBadge
      :name="labelText"
      :description="descriptionText"
      :hint="hintText"
      :kind="abilityKind"
      :group="groupKey"
      :source="groupKey"
      size="sm"
    />
    <div class="agent-tool-option-copy">
      <span class="agent-tool-option-name">{{ labelText }}</span>
      <span class="agent-tool-option-desc">{{ summaryText }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { useI18n } from '@/i18n';
import { isAbilitySkillGroup, resolveAbilitySummary } from '@/utils/abilityVisuals';

import AbilityIconBadge from '@/components/common/AbilityIconBadge.vue';

const props = withDefaults(
  defineProps<{
    label?: string;
    description?: string;
    hint?: string;
    groupKey?: string;
  }>(),
  {
    label: '',
    description: '',
    hint: '',
    groupKey: ''
  }
);

const { t } = useI18n();

const labelText = computed(() => String(props.label || '').trim());
const descriptionText = computed(() => String(props.description || '').trim());
const hintText = computed(() => String(props.hint || '').trim());
const abilityKind = computed<'tool' | 'skill'>(() =>
  isAbilitySkillGroup(props.groupKey) ? 'skill' : 'tool'
);
const summaryText = computed(() => {
  const summary = resolveAbilitySummary(descriptionText.value, hintText.value);
  return summary || t('chat.ability.noDesc');
});
const titleText = computed(() => {
  const parts = [labelText.value, descriptionText.value || hintText.value].filter(Boolean);
  return parts.join('\n');
});
</script>

<style scoped>
.agent-tool-option-label {
  width: 100%;
  min-width: 0;
  display: flex;
  align-items: flex-start;
  gap: 10px;
}

.agent-tool-option-copy {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.agent-tool-option-name,
.agent-tool-option-desc {
  display: -webkit-box;
  overflow: hidden;
  text-overflow: ellipsis;
  -webkit-box-orient: vertical;
  overflow-wrap: anywhere;
  word-break: break-word;
}

.agent-tool-option-name {
  color: var(--el-text-color-primary, #1f2937);
  font-size: 12px;
  font-weight: 700;
  line-height: 1.35;
  -webkit-line-clamp: 2;
}

.agent-tool-option-desc {
  color: var(--el-text-color-secondary, #64748b);
  font-size: 11px;
  line-height: 1.45;
  -webkit-line-clamp: 2;
}
</style>
