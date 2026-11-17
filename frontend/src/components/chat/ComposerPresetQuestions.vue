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
 * 预设问题列表：直接铺在输入区「+」面板的第一栏（一栏到底，点一下就填入草稿）。
 * 卡片外观与位置由父组件的 `.composer-panel--plus` 提供，这里只管列表自身，
 * 不借用 ChatComposer 的 scoped `.composer-panel`（scoped 样式不外溢到子组件内部）。
 */
defineProps({
  items: {
    type: Array as PropType<string[]>,
    default: () => []
  }
});

const emit = defineEmits(['pick']);
const { t } = useI18n();
</script>

<style scoped>
.composer-preset-panel {
  display: flex;
  flex-direction: column;
  gap: 1px;
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
  font-size: 12.5px;
  line-height: 1.35;
  text-align: left;
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
}

.composer-preset-panel-item:hover {
  background: var(--mz-hover, #f1efec);
  color: var(--mz-primary, var(--ui-accent-ink));
}

.composer-preset-panel-item:focus-visible {
  outline: 2px solid var(--mz-primary);
  outline-offset: -2px;
}
</style>
