<template>
  <div v-if="items.length" class="composer-preset-row" role="group" :aria-label="t('chat.commandMenu.presetQuestions')">
    <span class="composer-preset-label">
      <i class="fa-solid fa-wand-magic-sparkles composer-preset-label-icon" aria-hidden="true"></i>
      <span class="composer-preset-label-text">{{ t('chat.commandMenu.presetQuestions') }}</span>
    </span>
    <button
      v-for="item in items"
      :key="item"
      class="composer-preset-chip"
      type="button"
      :title="item"
      :disabled="disabled"
      @click="emit('pick', item)"
    >
      {{ item }}
    </button>
  </div>
</template>

<script setup lang="ts">
import type { PropType } from 'vue';

import { useI18n } from '@/i18n';

defineProps({
  items: {
    type: Array as PropType<string[]>,
    default: () => []
  },
  disabled: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits(['pick']);
const { t } = useI18n();
</script>

<style scoped>
.composer-preset-row {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  margin-bottom: 8px;
  overflow-x: auto;
  scrollbar-width: thin;
}

.composer-preset-label {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  flex-shrink: 0;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
  font-weight: 600;
}

.composer-preset-label-icon {
  font-size: 11px;
}

.composer-preset-chip {
  flex-shrink: 0;
  max-width: 260px;
  padding: 4px 10px;
  overflow: hidden;
  border: 1px solid var(--mz-border, #e8e6e3);
  border-radius: 999px;
  background: var(--mz-surface, #ffffff);
  color: var(--mz-text-secondary, #3d3d3d);
  font: inherit;
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
  transition: border-color 120ms ease, color 120ms ease, background-color 120ms ease;
}

.composer-preset-chip:hover:not(:disabled) {
  border-color: var(--mz-primary-soft, #dfac9a);
  background: var(--mz-primary-tint, #f6e9e3);
  color: var(--mz-primary, #c96443);
}

.composer-preset-chip:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.composer-preset-chip:focus-visible {
  outline: 2px solid var(--mz-primary-soft, #dfac9a);
  outline-offset: 1px;
}

@media (max-width: 560px) {
  .composer-preset-label-text {
    display: none;
  }
}
</style>
