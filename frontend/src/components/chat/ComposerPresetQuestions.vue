<template>
  <div
    class="composer-preset-panel"
    role="group"
    :aria-label="t('chat.commandMenu.presetQuestions')"
    @click.stop
  >
    <div class="composer-preset-panel-title">{{ t('chat.commandMenu.presetQuestions') }}</div>
    <button
      v-for="item in items"
      :key="item"
      class="composer-preset-panel-item"
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

/**
 * 预设问题浮层：由输入卡工具栏的魔法棒按钮开合，选中项填入草稿。
 * 样式自带（不借用 ChatComposer 的 scoped `.composer-panel`，scoped 样式不外溢到子组件）。
 */
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
.composer-preset-panel {
  position: absolute;
  left: 0;
  bottom: calc(100% + 8px);
  z-index: 40;
  display: flex;
  flex-direction: column;
  gap: 1px;
  box-sizing: border-box;
  width: 268px;
  max-width: calc(100vw - 40px);
  max-height: 300px;
  padding: 6px;
  overflow-y: auto;
  border: 1px solid var(--mz-border, #e8e6e3);
  border-radius: 12px;
  background: var(--mz-surface, #ffffff);
  box-shadow: 0 12px 32px rgba(31, 35, 41, 0.16);
  scrollbar-width: thin;
}

.composer-preset-panel-title {
  padding: 4px 8px 6px;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
  font-weight: 600;
}

.composer-preset-panel-item {
  flex-shrink: 0;
  width: 100%;
  padding: 7px 8px;
  overflow: hidden;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--mz-text, #1f2329);
  font: inherit;
  font-size: 13px;
  text-align: left;
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
}

.composer-preset-panel-item:hover:not(:disabled) {
  background: var(--mz-hover, #f1efec);
  color: var(--mz-primary, #c96443);
}

.composer-preset-panel-item:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.composer-preset-panel-item:focus-visible {
  outline: 2px solid var(--mz-primary-soft, #dfac9a);
  outline-offset: -2px;
}
</style>
