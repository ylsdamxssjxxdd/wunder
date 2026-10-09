<template>
  <div
    class="composer-preset-panel"
    role="group"
    :aria-label="t('chat.commandMenu.presetQuestions')"
    @click.stop
  >
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
 * 预设问题列表：作为输入区「+」菜单的级联子面板渲染（悬停行从右侧呼出）。
 * 卡片外观与定位由父组件的 `.composer-plus-flyout` 提供（父级 scoped 样式会落到子组件根节点），
 * 这里只管列表自身，不借用 ChatComposer 的 scoped `.composer-panel`（scoped 样式不外溢到子组件内部）。
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
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.composer-preset-panel-item {
  flex-shrink: 0;
  width: 100%;
  padding: 6px 8px;
  overflow: hidden;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--mz-text-secondary, #3d3d3d);
  font: inherit;
  font-size: 12px;
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
