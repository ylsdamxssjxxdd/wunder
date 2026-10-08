<template>
  <el-dialog
    :model-value="visible"
    class="system-prompt-dialog"
    :title="t('chat.systemPrompt.title')"
    width="720px"
    top="clamp(10px, 4vh, 36px)"
    append-to-body
    @update:model-value="handleVisibleChange"
  >
    <div v-if="loading" class="messenger-list-empty">{{ t('chat.systemPrompt.loading') }}</div>
    <template v-else>
      <section class="system-prompt-full-panel" :class="`system-prompt-full-panel--${memoryMode}`">
        <div v-if="statusHint" class="system-prompt-memory-hint muted">
          {{ statusHint }}
        </div>
        <div v-if="hasToolingContent" class="system-prompt-view-switch">
          <button
            class="system-prompt-view-btn"
            :class="{ 'is-active': activeView === 'prompt' }"
            type="button"
            @click="activeView = 'prompt'"
          >
            {{ t('chat.systemPrompt.viewPrompt') }}
          </button>
          <button
            class="system-prompt-view-btn"
            :class="{ 'is-active': activeView === 'tooling' }"
            type="button"
            @click="activeView = 'tooling'"
          >
            {{ t('chat.systemPrompt.viewTooling') }}
          </button>
          <span class="system-prompt-tooling-mode">
            {{ t('chat.systemPrompt.toolingMode', { mode: toolingModeLabel }) }}
          </span>
        </div>
        <pre
          v-if="!hasToolingContent || activeView === 'prompt'"
          class="workflow-dialog-detail system-prompt-content"
          v-html="htmlContent"
        ></pre>
        <div
          v-else
          class="workflow-dialog-detail system-prompt-tooling-content"
        >
          <PromptToolingPreviewList
            :items="toolingItems"
            :fallback-text="toolingContent"
          />
        </div>
      </section>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';

import PromptToolingPreviewList from '@/components/chat/PromptToolingPreviewList.vue';
import { useI18n } from '@/i18n';
import type { PromptToolingPreviewItem } from '@/utils/promptToolingPreview';

const props = defineProps<{
  visible: boolean;
  loading: boolean;
  htmlContent: string;
  memoryMode: 'none' | 'pending' | 'frozen';
  toolingMode?: string;
  toolingContent?: string;
  toolingItems?: PromptToolingPreviewItem[];
}>();

const emit = defineEmits<{
  'update:visible': [value: boolean];
}>();

const { t } = useI18n();

const statusHint = computed(() => {
  if (props.memoryMode === 'frozen') {
    return t('chat.systemPrompt.memoryFrozenHint');
  }
  if (props.memoryMode === 'pending') {
    return t('chat.systemPrompt.memoryPendingHint');
  }
  return '';
});

const toolingItems = computed(() =>
  Array.isArray(props.toolingItems) ? props.toolingItems.filter((item) => item?.name) : []
);
const hasToolingContent = computed(
  () => toolingItems.value.length > 0 || String(props.toolingContent || '').trim().length > 0
);
const activeView = ref<'prompt' | 'tooling'>('prompt');

const toolingModeLabel = computed(() => {
  const mode = String(props.toolingMode || '').trim().toLowerCase();
  if (!mode) {
    return '-';
  }
  const key = `chat.systemPrompt.toolCallMode.${mode}`;
  const translated = t(key);
  return translated === key ? mode : translated;
});

watch(
  () => props.visible,
  (visible) => {
    if (!visible) {
      activeView.value = 'prompt';
    }
  }
);

watch(
  () => [props.toolingContent, props.toolingItems],
  () => {
    activeView.value = 'prompt';
  },
  { deep: true }
);

const handleVisibleChange = (nextVisible: boolean) => {
  emit('update:visible', Boolean(nextVisible));
};
</script>

<style scoped>
.system-prompt-full-panel {
  border: 1px solid var(--app-border-color, rgba(148, 163, 184, 0.24));
  border-radius: 14px;
  background: var(--app-panel-bg, rgba(15, 23, 42, 0.04));
  padding: 14px;
  min-height: 0;
  height: 100%;
  max-height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.system-prompt-full-panel--frozen {
  border-color: rgba(59, 130, 246, 0.32);
}

.system-prompt-full-panel--pending {
  border-color: rgba(245, 158, 11, 0.32);
}

.system-prompt-memory-hint {
  margin-bottom: 12px;
  line-height: 1.7;
}

.system-prompt-content {
  margin: 0;
  flex: 1 1 auto;
  min-height: 0;
  max-height: none;
  overflow: auto;
}

.system-prompt-view-switch {
  margin-bottom: 10px;
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px;
}

.system-prompt-view-btn {
  border: 1px solid var(--app-border-color, rgba(148, 163, 184, 0.24));
  background: var(--app-panel-bg, rgba(148, 163, 184, 0.1));
  color: var(--app-text-color, #0f172a);
  border-radius: 8px;
  padding: 6px 10px;
  font-size: 12px;
  line-height: 1.2;
  display: inline-flex;
  align-items: center;
  cursor: pointer;
}

.system-prompt-view-btn.is-active {
  border-color: rgba(59, 130, 246, 0.36);
  background: rgba(59, 130, 246, 0.14);
  color: var(--app-text-color, #0f172a);
}

.system-prompt-tooling-mode {
  margin-left: auto;
  border-radius: 999px;
  border: 1px solid var(--app-border-color, rgba(148, 163, 184, 0.24));
  padding: 2px 8px;
  font-size: 11px;
  color: var(--app-text-color, #0f172a);
}

.system-prompt-tooling-content {
  margin: 0;
  flex: 1 1 auto;
  min-height: 0;
  max-height: none;
  overflow: auto;
  white-space: normal;
}
</style>
